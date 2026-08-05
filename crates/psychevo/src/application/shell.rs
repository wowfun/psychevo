use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Client, TurnHandle};
use crate::types::{
    RunControl, RunControlHandle, RunMode, RunStreamEvent, RunStreamSink, UserShellContextOptions,
    UserShellOptions, run_control,
};
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct ShellCommandRequest {
    cwd: PathBuf,
    command: String,
    thread_id: Option<String>,
    continue_latest: bool,
    continue_sources: Vec<String>,
    source: String,
    model: Option<String>,
    reasoning_effort: Option<String>,
    mode: RunMode,
    inherited_env: Option<BTreeMap<String, String>>,
    inject_into: Option<TurnHandle>,
    persist: bool,
}

impl ShellCommandRequest {
    pub fn new(cwd: impl Into<PathBuf>, command: impl Into<String>) -> Self {
        Self {
            cwd: cwd.into(),
            command: command.into(),
            thread_id: None,
            continue_latest: false,
            continue_sources: Vec::new(),
            source: "shell".to_string(),
            model: None,
            reasoning_effort: None,
            mode: RunMode::Default,
            inherited_env: None,
            inject_into: None,
            persist: true,
        }
    }

    pub fn thread(mut self, thread_id: impl Into<String>) -> Self {
        self.thread_id = Some(thread_id.into());
        self.continue_latest = false;
        self
    }

    pub fn continue_latest(mut self, sources: impl IntoIterator<Item = String>) -> Self {
        self.thread_id = None;
        self.continue_latest = true;
        self.continue_sources = sources.into_iter().collect();
        self
    }

    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    pub fn model(mut self, model: Option<String>, reasoning_effort: Option<String>) -> Self {
        self.model = model;
        self.reasoning_effort = reasoning_effort;
        self
    }

    pub fn mode(mut self, mode: RunMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn inherited_environment(mut self, environment: BTreeMap<String, String>) -> Self {
        self.inherited_env = Some(environment);
        self
    }

    pub fn inject_into(mut self, turn: TurnHandle) -> Self {
        self.inject_into = Some(turn);
        self
    }

    pub fn transient(mut self) -> Self {
        self.persist = false;
        self.thread_id = None;
        self.continue_latest = false;
        self.continue_sources.clear();
        self.inject_into = None;
        self
    }
}

pub struct ShellCommand {
    client: Client,
    request: ShellCommandRequest,
    control_handle: RunControlHandle,
    control: RunControl,
}

impl fmt::Debug for ShellCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ShellCommand")
            .field("cwd", &self.request.cwd)
            .field("thread_id", &self.request.thread_id)
            .field("continue_latest", &self.request.continue_latest)
            .field("persist", &self.request.persist)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct ShellCommandControl {
    handle: RunControlHandle,
}

impl ShellCommandControl {
    pub fn interrupt(&self) {
        self.handle.abort();
    }

    pub fn is_interrupted(&self) -> bool {
        self.handle.inner.is_aborted()
    }
}

impl fmt::Debug for ShellCommandControl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ShellCommandControl(..)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellCommandOutcome {
    Completed,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ShellCommandEvent {
    Started {
        thread_id: Option<String>,
        command: String,
        started_at_ms: i64,
    },
    Completed {
        thread_id: Option<String>,
        output: Value,
        outcome: ShellCommandOutcome,
        elapsed_ms: u64,
    },
    Warning {
        kind: String,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShellCommandResult {
    pub command: String,
    pub cwd: PathBuf,
    pub thread_id: Option<String>,
    pub context_text: Option<String>,
    pub outcome: ShellCommandOutcome,
    pub tool_failures: usize,
    pub output: Value,
}

impl Client {
    pub fn shell_command(&self, mut request: ShellCommandRequest) -> Result<ShellCommand> {
        self.ensure_open()?;
        request.cwd = crate::paths::canonicalize_cwd(&request.cwd)?;
        if request.command.trim().is_empty() {
            return Err(Error::Message("shell command is empty".to_string()));
        }
        request.source = request.source.trim().to_string();
        if request.persist && request.source.is_empty() {
            return Err(Error::Message("shell source is empty".to_string()));
        }
        let (control_handle, control) = run_control();
        Ok(ShellCommand {
            client: self.clone(),
            request,
            control_handle,
            control,
        })
    }
}

impl ShellCommand {
    pub fn control(&self) -> ShellCommandControl {
        ShellCommandControl {
            handle: self.control_handle.clone(),
        }
    }

    pub async fn run(
        self,
        emit: impl Fn(ShellCommandEvent) + Send + Sync + 'static,
    ) -> Result<ShellCommandResult> {
        self.client.ensure_open()?;
        let emit = Arc::new(emit);
        let event_emit = Arc::clone(&emit);
        let stream: RunStreamSink = Arc::new(move |event| {
            if let Some(event) = typed_shell_event(&event) {
                event_emit(event);
            }
        });
        let environment = self
            .client
            .application_environment(self.request.inherited_env);
        let context = self.request.persist.then(|| UserShellContextOptions {
            state: self.client.inner.state.clone(),
            session: self.request.thread_id,
            continue_latest: self.request.continue_latest,
            source: self.request.source,
            continue_sources: self.request.continue_sources,
            config_path: self.client.inner.config_path.clone(),
            model: self.request.model,
            reasoning_effort: self.request.reasoning_effort,
            mode: self.request.mode,
        });
        let result = crate::user_shell::run_user_shell_command_streaming_controlled(
            UserShellOptions {
                cwd: self.request.cwd,
                command: self.request.command,
                environment,
                context,
                inject_into: self.request.inject_into.map(|turn| turn.control),
            },
            stream,
            self.control,
        )
        .await?;
        Ok(ShellCommandResult {
            command: result.command,
            cwd: result.cwd,
            thread_id: result.session_id,
            context_text: result.context_text,
            outcome: shell_outcome(result.outcome),
            tool_failures: result.tool_failures,
            output: result.result,
        })
    }
}

fn typed_shell_event(event: &RunStreamEvent) -> Option<ShellCommandEvent> {
    let value = event.legacy_value()?;
    match value.get("type").and_then(Value::as_str) {
        Some("tool_execution_start") => Some(ShellCommandEvent::Started {
            thread_id: optional_string(value.get("session_id")),
            command: value
                .pointer("/args/cmd")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            started_at_ms: value
                .get("started_at_ms")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
        }),
        Some("tool_execution_end") => Some(ShellCommandEvent::Completed {
            thread_id: optional_string(value.get("session_id")),
            output: value.get("result").cloned().unwrap_or(Value::Null),
            outcome: match value.get("outcome").and_then(Value::as_str) {
                Some("normal") => ShellCommandOutcome::Completed,
                Some("stopped" | "aborted") => ShellCommandOutcome::Interrupted,
                _ => ShellCommandOutcome::Failed,
            },
            elapsed_ms: value
                .get("elapsed_ms")
                .and_then(Value::as_u64)
                .unwrap_or_default(),
        }),
        Some("warning") => Some(ShellCommandEvent::Warning {
            kind: value
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("shell")
                .to_string(),
            message: value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        }),
        _ => None,
    }
}

fn optional_string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
}

fn shell_outcome(outcome: psychevo_ai::Outcome) -> ShellCommandOutcome {
    match outcome {
        psychevo_ai::Outcome::Normal => ShellCommandOutcome::Completed,
        psychevo_ai::Outcome::Failed => ShellCommandOutcome::Failed,
        psychevo_ai::Outcome::Stopped | psychevo_ai::Outcome::Aborted => {
            ShellCommandOutcome::Interrupted
        }
    }
}
