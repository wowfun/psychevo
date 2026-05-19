use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use psychevo_agent_core::user_text_message;
use psychevo_ai::Outcome;
use serde_json::{Map, Value, json};

use crate::config::{load_run_config, resolve_run_provider};
use crate::error::{Error, Result};
use crate::paths::canonical_workdir;
use crate::run::SESSION_TITLE_MAX_CHARS;
use crate::store::SqliteStore;
use crate::tools::{default_bash_timeout_secs, run_bash_command};
use crate::types::{
    RunControl, RunOptions, RunStreamEvent, RunStreamSink, USER_SHELL_METADATA_KEY,
    UserShellContextOptions, UserShellOptions, UserShellResult,
};

struct PreparedUserShellContext {
    store: SqliteStore,
    session_id: String,
}

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
    let prepared_context = options
        .context
        .as_ref()
        .map(|context| prepare_user_shell_context(context, &workdir, &command))
        .transpose()?;
    let stream_session_id = prepared_context
        .as_ref()
        .map(|context| context.session_id.clone());

    let tool_call_id = "user_shell".to_string();
    stream(RunStreamEvent::Event(json!({
        "type": "tool_execution_start",
        "session_id": stream_session_id.clone(),
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
    let elapsed = started.elapsed();
    stream(RunStreamEvent::Event(json!({
        "type": "tool_execution_end",
        "session_id": stream_session_id,
        "tool_call_id": tool_call_id,
        "tool_name": "bash",
        "result": result.clone(),
        "outcome": outcome.as_str(),
        "elapsed_ms": elapsed.as_millis() as u64,
        "source": "user_shell",
    })));

    let (session_id, context_text) = if let Some(prepared_context) = prepared_context {
        let context_text = user_shell_context_text(&command, &result, elapsed);
        let message = user_text_message(context_text.clone());
        prepared_context
            .store
            .append_message_with_undo_snapshot_metadata_and_context_evidence(
                &prepared_context.session_id,
                &message,
                Some(user_shell_metadata(
                    &command, &workdir, outcome, is_error, elapsed, &result,
                )),
                Some(format!("!{command}")),
                &[],
            )?;
        if let Some(handle) = options.inject_into {
            let _ = handle.inject_user_message(message);
        }
        (Some(prepared_context.session_id), Some(context_text))
    } else {
        (None, None)
    };

    Ok(UserShellResult {
        command,
        workdir,
        session_id,
        context_text,
        outcome,
        tool_failures: usize::from(is_error && !aborted),
        result,
    })
}

fn prepare_user_shell_context(
    context: &UserShellContextOptions,
    workdir: &Path,
    command: &str,
) -> Result<PreparedUserShellContext> {
    let options = RunOptions {
        db_path: context.db_path.clone(),
        workdir: workdir.to_path_buf(),
        snapshot_root: None,
        session: context.session.clone(),
        continue_latest: context.continue_latest,
        prompt: command.to_string(),
        image_inputs: Vec::new(),
        extract_prompt_image_sources: false,
        prompt_display: None,
        max_context_messages: None,
        config_path: context.config_path.clone(),
        model: context.model.clone(),
        reasoning_effort: context.reasoning_effort.clone(),
        include_reasoning: false,
        mode: context.mode,
        permission_mode: None,
        approval_mode: None,
        approval_handler: None,
        inherited_env: context.inherited_env.clone(),
        agent: None,
        no_agents: true,
        no_skills: true,
        skill_inputs: Vec::new(),
    };
    let loaded = load_run_config(&options, workdir)?;
    let resolved = resolve_run_provider(&options, &loaded)?;
    let store = SqliteStore::open(&context.db_path)?;
    let continue_sources = context
        .continue_sources
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let (session_id, created_session) = if let Some(session_id) = context.session.clone() {
        store.resume_session(&session_id)?;
        (session_id, false)
    } else if context.continue_latest {
        if let Some(session_id) =
            store.latest_session_for_workdir_with_sources(workdir, &continue_sources)?
        {
            store.resume_session(&session_id)?;
            (session_id, false)
        } else {
            (
                store.create_session_with_metadata(
                    workdir,
                    &context.source,
                    &resolved.model,
                    &resolved.provider,
                    Some(json!({
                        "provider_label": resolved.display_label.clone(),
                        "base_url": resolved.base_url.clone(),
                        "api_key_env": resolved.api_key_env.clone(),
                        "reasoning_effort": resolved.reasoning_effort.clone(),
                        "context_limit": resolved.context_limit,
                        "model_metadata": resolved.metadata.public_json(),
                        "mode": context.mode.as_str(),
                    })),
                )?,
                true,
            )
        }
    } else {
        (
            store.create_session_with_metadata(
                workdir,
                &context.source,
                &resolved.model,
                &resolved.provider,
                Some(json!({
                    "provider_label": resolved.display_label.clone(),
                    "base_url": resolved.base_url.clone(),
                    "api_key_env": resolved.api_key_env.clone(),
                    "reasoning_effort": resolved.reasoning_effort.clone(),
                    "context_limit": resolved.context_limit,
                    "model_metadata": resolved.metadata.public_json(),
                    "mode": context.mode.as_str(),
                })),
            )?,
            true,
        )
    };
    if created_session {
        let title = deterministic_shell_session_title(command);
        store.set_session_title(&session_id, &title)?;
    }
    Ok(PreparedUserShellContext { store, session_id })
}

fn user_shell_metadata(
    command: &str,
    workdir: &Path,
    outcome: Outcome,
    is_error: bool,
    elapsed: Duration,
    result: &Value,
) -> Value {
    let mut metadata = Map::new();
    metadata.insert(
        USER_SHELL_METADATA_KEY.to_string(),
        json!({
            "command": command,
            "workdir": workdir,
            "outcome": outcome.as_str(),
            "is_error": is_error,
            "exit_code": result.get("exit_code").cloned().unwrap_or(Value::Null),
            "truncated": result.get("truncated").and_then(Value::as_bool).unwrap_or(false),
            "duration_seconds": elapsed.as_secs_f64(),
            "elapsed_ms": elapsed.as_millis() as u64,
            "result": result,
        }),
    );
    Value::Object(metadata)
}

pub(crate) fn user_shell_context_text(command: &str, result: &Value, elapsed: Duration) -> String {
    let exit_code = result
        .get("exit_code")
        .map(context_scalar)
        .unwrap_or_else(|| "null".to_string());
    let truncated = result
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let error = result
        .get("error")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let output = result_output(result);
    let mut result_lines = vec![
        format!("Exit code: {exit_code}"),
        format!("Duration: {:.3} seconds", elapsed.as_secs_f64()),
        format!("Truncated: {truncated}"),
    ];
    if let Some(error) = error {
        result_lines.push(format!("Error: {}", escape_xml_text(error)));
    }
    result_lines.push(format!("Output:\n{}", escape_xml_text(output)));
    format!(
        "<user_shell_command><command>{}</command><result>{}</result></user_shell_command>",
        escape_xml_text(command),
        result_lines.join("\n")
    )
}

fn result_output(result: &Value) -> &str {
    result
        .get("output")
        .and_then(Value::as_str)
        .or_else(|| result.get("content").and_then(Value::as_str))
        .or_else(|| result.get("error").and_then(Value::as_str))
        .filter(|value| !value.is_empty())
        .unwrap_or("(no output)")
}

fn context_scalar(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

fn deterministic_shell_session_title(command: &str) -> String {
    let first_line = command
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("command");
    truncate_chars(&format!("Shell: {first_line}"), SESSION_TITLE_MAX_CHARS)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect()
}

fn escape_xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}
