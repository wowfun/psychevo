use std::sync::Mutex;

use psychevo::{
    Error, ShellCommandControl, ShellCommandOutcome, ShellCommandRequest,
    application::{GatewayActivityKind, GatewayActivityTerminalStatus},
};
use serde_json::{Value, json};

use super::Gateway;
use super::activity::{ActiveActivityControl, ActiveActivityKind, SendShellRequest};
use super::agent_session::agent_error_view;
use super::durable_activity::{DurableGatewayActivity, DurableGatewayActivityClaim};
use super::results::GatewayShellResult;
use super::stream_input::source_key_key;
use crate::journey_profile::{self, gateway_profile_mark};
use crate::projection::GatewayLiveProjector;
use crate::transcript;
use psychevo_gateway_protocol::events_transcript::GatewayEvent;
use psychevo_gateway_protocol::source::{
    BackendKind, GatewayBackendInfo, GatewaySource, GatewayThread, GatewayTurnError,
};

impl Gateway {
    pub(super) async fn run_shell_now(
        &self,
        queue_key: &str,
        request: SendShellRequest,
        shell_id: String,
    ) -> psychevo::Result<GatewayShellExecution> {
        self.run_shell(request, shell_id, Some(queue_key)).await
    }

    async fn run_shell(
        &self,
        mut request: SendShellRequest,
        shell_id: String,
        queue_key: Option<&str>,
    ) -> psychevo::Result<GatewayShellExecution> {
        let queue_source = request.source.clone();
        let bind_source = request.bind_source.clone().or_else(|| queue_source.clone());
        let bind_source_generation = bind_source
            .as_ref()
            .map(|source| self.source_generation(source));
        let queue_source_generation = queue_source
            .as_ref()
            .map(|source| self.source_generation(source));
        let mut execution = request.execution;
        let explicit_thread = request.thread_id.is_some();
        let source_thread_id = if let Some(source) = request.source.as_ref() {
            self.lookup_source_thread(source).await.ok().flatten()
        } else {
            None
        };
        let active_thread_id = request.thread_id.clone().or(source_thread_id);
        let active_thread = match active_thread_id.as_deref() {
            Some(thread_id) => Some(self.framework_thread(thread_id).await?),
            None => None,
        };
        if let Some(thread) = active_thread.as_ref() {
            request.cwd = thread.summary().await?.cwd.into();
            execution.continue_latest = false;
        }
        let mut framework_request =
            ShellCommandRequest::new(request.cwd.clone(), request.command.clone())
                .source(execution.runtime_source.clone())
                .model(execution.model.take(), execution.reasoning_effort.take())
                .mode(execution.mode);
        if let Some(thread_id) = active_thread_id.as_ref() {
            framework_request = framework_request.thread(thread_id.clone());
        } else if execution.continue_latest {
            framework_request =
                framework_request.continue_latest(execution.continue_sources.clone());
        }
        if let Some(environment) = execution.inherited_env.take() {
            framework_request = framework_request.inherited_environment(environment);
        }
        let shell_command = self.framework_client().shell_command(framework_request)?;
        let shell_control = shell_command.control();
        if let Some(queue_key) = queue_key {
            self.register_active(
                queue_key,
                shell_id.clone(),
                Some(ActiveActivityControl::Shell(shell_control.clone())),
                ActiveActivityKind::Shell,
            );
            if request.thread_id.is_none()
                && let Some(source) = &request.source
            {
                self.register_active_queue_alias(&source_key_key(&source.source_key()), queue_key);
            }
        }
        let mut cancellation = ShellCancellationGuard::new(shell_control);
        let durable_source_key = if explicit_thread {
            None
        } else {
            queue_source
                .as_ref()
                .or(bind_source.as_ref())
                .map(|source| source.source_key().0)
        };
        let first_committed_seq = match active_thread.as_ref() {
            Some(thread) => thread
                .history()
                .latest(Some(1))
                .await?
                .items
                .last()
                .map(|item| item.session_seq.saturating_add(1))
                .unwrap_or(1),
            None => 1,
        };
        let durable_intent = json!({
            "kind": "shell",
            "threadId": active_thread_id.clone(),
            "sourceKey": durable_source_key.clone(),
            "runtimeSource": execution.runtime_source.clone(),
            "firstCommittedSeq": first_committed_seq,
            "cwd": request.cwd.to_string_lossy(),
            "command": request.command.clone(),
        });
        let durable_activity = self
            .claim_durable_gateway_activity(DurableGatewayActivityClaim {
                activity_id: &shell_id,
                thread_id: active_thread_id.as_deref(),
                source_key: durable_source_key.as_deref(),
                turn_id: Some(&shell_id),
                kind: GatewayActivityKind::Shell,
                owner_surface: Some(&execution.runtime_source),
                queued_turns: 0,
                intent: Some(durable_intent),
            })
            .await?;
        let lease_lost = self.track_shell_activity(durable_activity.clone());
        let event_activity = durable_activity.clone();
        let execution = async {
            let event_sink = self.wrap_gateway_event_sink(
                request.event_sink.clone(),
                Some(event_activity),
                queue_key.map(str::to_string),
                Some(shell_id.clone()),
            );
            let event_sink_for_completion = event_sink.clone();
            let shell_event_id = shell_id.clone();
            let projection = event_sink.map(|event_sink| {
                (
                    Mutex::new(GatewayLiveProjector::new(active_thread_id.clone())),
                    event_sink,
                )
            });
            let projection_turn_id = shell_id;
            let result = shell_command
                .run(move |event| {
                    let Some((projector, event_sink)) = &projection else {
                        return;
                    };
                    let event = projector
                        .lock()
                        .expect("gateway live projector poisoned")
                        .project_shell_event(&projection_turn_id, &event);
                    let fields = journey_profile::gateway_profile_event_fields(&event);
                    gateway_profile_mark(
                        "gateway_event_emitted",
                        Some(&projection_turn_id),
                        match &event {
                            GatewayEvent::EntryStarted { entry, .. }
                            | GatewayEvent::EntryUpdated { entry, .. }
                            | GatewayEvent::EntryCompleted { entry, .. } => {
                                Some(entry.thread_id.as_str())
                            }
                            GatewayEvent::EntryBlockTextDelta { thread_id, .. } => {
                                thread_id.as_deref()
                            }
                            _ => None,
                        },
                        fields,
                    );
                    let _ = event_sink.emit(event);
                })
                .await?;
            let session_id = result
                .thread_id
                .clone()
                .or_else(|| active_thread_id.clone())
                .ok_or_else(|| {
                    Error::Message("shell command did not resolve a session".to_string())
                })?;
            let backend = GatewayBackendInfo {
                kind: BackendKind::Native,
                runtime_ref: Some("native".to_string()),
                native_id: Some(session_id.clone()),
            };
            if let Some(source) = &bind_source
                && bind_source_generation
                    .is_none_or(|generation| self.source_generation(source) == generation)
            {
                self.bind_source_thread(source, &session_id, &backend, request.lineage)
                    .await?;
            }
            if let Some(source) = &queue_source
                && bind_source
                    .as_ref()
                    .is_none_or(|bind_source| bind_source.source_key() != source.source_key())
                && queue_source_generation
                    .is_none_or(|generation| self.source_generation(source) == generation)
            {
                self.bind_source_thread(source, &session_id, &backend, None)
                    .await?;
            }
            let history = self.framework_thread(&session_id).await?.history();
            let mut summaries = Vec::new();
            let mut after = Some(first_committed_seq.saturating_sub(1));
            loop {
                let page = history.replay_after(after, Some(200)).await?;
                if let Some(warning) = page.warnings.first() {
                    return Err(Error::Message(format!(
                        "committed Shell history is invalid at message {} ({:?})",
                        warning.session_seq, warning.kind
                    )));
                }
                summaries.extend(page.items.into_iter().filter_map(|item| match item {
                    psychevo::application::HistoryReplayItem::Available { item } => Some(*item),
                    psychevo::application::HistoryReplayItem::Unavailable { .. } => None,
                }));
                let Some(next_after) = page.next_after else {
                    break;
                };
                after = Some(next_after);
            }
            let committed_entries = transcript::project_committed_turn_window_entries(
                &session_id,
                &summaries,
                transcript::TurnProjectionWindow {
                    turn_id: &shell_event_id,
                    first_committed_seq,
                },
            );
            if let Some(event_sink) = event_sink_for_completion {
                for entry in committed_entries.clone() {
                    let _ = event_sink.emit(GatewayEvent::EntryUpdated {
                        turn_id: shell_event_id.clone(),
                        entry,
                    });
                }
            }
            Ok(GatewayShellResult {
                thread: GatewayThread {
                    id: session_id,
                    backend,
                    source_key: bind_source.as_ref().map(GatewaySource::source_key),
                    forked_from_thread_id: None,
                },
                result,
                committed_entries,
            })
        };
        tokio::pin!(execution);
        let (result, execution_settled) = tokio::select! {
            result = &mut execution => (result, true),
            _ = lease_lost.cancelled() => (Err(Error::Message(format!(
                "Gateway Shell activity `{}` lost its durable lease",
                durable_activity.activity_id
            ))), false),
        };
        if execution_settled {
            cancellation.disarm();
        }
        self.untrack_shell_activity(&durable_activity.activity_id);
        let status = match &result {
            Ok(result) => gateway_activity_status_for_shell_outcome(result.result.outcome),
            Err(_) if lease_lost.is_cancelled() => GatewayActivityTerminalStatus::Interrupted,
            Err(_) => GatewayActivityTerminalStatus::Failed,
        };
        Ok(GatewayShellExecution {
            activity: durable_activity,
            result,
            status,
        })
    }

    pub(super) async fn finalize_shell_execution(
        &self,
        execution: GatewayShellExecution,
    ) -> psychevo::Result<GatewayShellResult> {
        let finished = self
            .finish_durable_gateway_activity(Some(&execution.activity), execution.status)
            .await;
        if let Err(error) = &finished {
            self.shell_activity_runtime.record_failure(format!(
                "Gateway Shell activity `{}` finalization failed: {error}",
                execution.activity.activity_id
            ));
        }
        match (execution.result, finished) {
            (Ok(result), Ok(())) => Ok(result),
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(error)) => Err(Error::Message(format!(
                "Gateway Shell activity finalization failed: {error}"
            ))),
            (Err(error), Err(finish_error)) => Err(Error::Message(format!(
                "{error}; Gateway Shell finalization also failed: {finish_error}"
            ))),
        }
    }
}

pub(super) struct GatewayShellExecution {
    activity: DurableGatewayActivity,
    result: psychevo::Result<GatewayShellResult>,
    status: GatewayActivityTerminalStatus,
}

struct ShellCancellationGuard(Option<ShellCommandControl>);

impl ShellCancellationGuard {
    fn new(control: ShellCommandControl) -> Self {
        Self(Some(control))
    }

    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for ShellCancellationGuard {
    fn drop(&mut self) {
        if let Some(control) = self.0.take() {
            control.interrupt();
        }
    }
}

pub(super) fn gateway_activity_status_for_shell_outcome(
    outcome: ShellCommandOutcome,
) -> GatewayActivityTerminalStatus {
    match outcome {
        ShellCommandOutcome::Completed => GatewayActivityTerminalStatus::Completed,
        ShellCommandOutcome::Failed => GatewayActivityTerminalStatus::Failed,
        ShellCommandOutcome::Interrupted => GatewayActivityTerminalStatus::Interrupted,
    }
}

pub(crate) fn unavailable_compaction_result(
    thread_id: &str,
    reason: psychevo::compaction::CompactionReason,
    runtime_ref: &str,
) -> psychevo::compaction::CompactionResult {
    psychevo::compaction::CompactionResult {
        session_id: thread_id.to_string(),
        compacted: false,
        reason: reason.as_str().to_string(),
        message: format!(
            "Context compaction is unavailable for runtime profile `{runtime_ref}` until its adapter owns native compaction."
        ),
        checkpoint_id: None,
        first_kept_session_seq: None,
        tokens_before: None,
        tokens_after: None,
        summary: None,
        summary_provider: None,
        summary_model: None,
    }
}

pub(super) fn gateway_turn_error(message: &str, data: Option<&Value>) -> GatewayTurnError {
    let mut error = agent_error_view(message, data);
    error.stage = error.stage.or_else(|| data.map(|_| "prompt".to_string()));
    error.retry_class = error
        .retry_class
        .or_else(|| data.map(|_| "never".to_string()));
    error
}
