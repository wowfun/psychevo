use std::path::PathBuf;
use std::time::Duration;

use psychevo::Error;
use psychevo::application::{
    AutomationRunFinishInput, AutomationRunRecord, AutomationRunTerminalStatus,
    AutomationTaskRecord,
};
use psychevo::automations::latest_due_at_ms;
use psychevo::{PermissionMode, RunSandboxOverride};
use psychevo_gateway_protocol as wire;
use serde_json::json;
use uuid::Uuid;

use super::support::{
    automation_execution_from_value, automation_kind_to_wire, automation_schedule_from_value,
    automation_source, next_run_after_now,
};
use crate::gateway::activity::ThreadSurface;
use crate::gateway::results::GatewayTurnResult;
use crate::gateway_now_ms;
use crate::server::binding::WebState;
use crate::server::event_delivery::ConnectionSender;
use crate::server::thread_application::framework_gateway_turn_result;
use psychevo_gateway_protocol::source::{
    BackendKind, GatewayBackendInfo, GatewayInputPart, GatewayThreadSelector,
};

const AUTOMATION_DUE_LIMIT: usize = 10;
const AUTOMATION_SCHEDULER_TICK_MS: u64 = 30_000;
const AUTOMATION_STALE_RUN_RECOVERY_MS: i64 = 5 * 60 * 1000;
const AUTOMATION_STALE_RUN_RECOVERY_LIMIT: usize = 50;
const AUTOMATION_STALE_RUN_RECOVERY_ERROR: &str =
    "automation run recovery: stale running claim expired without an active gateway activity";

pub(in crate::server) fn reconcile(state: WebState) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    let gateway = state.inner.gateway.clone();
    gateway.spawn_background("automation-scheduler", async move {
        if let Err(err) = recover_stale_automation_runs(&state).await {
            eprintln!("automation stale-run recovery failed: {err}");
        }
        let mut tick = tokio::time::interval(Duration::from_millis(AUTOMATION_SCHEDULER_TICK_MS));
        loop {
            tick.tick().await;
            if let Err(err) = run_due_automations_once(state.clone()).await {
                eprintln!("automation scheduler failed: {err}");
            }
        }
    });
}

pub(in crate::server) async fn run_due_automations_once(
    state: WebState,
) -> psychevo::Result<usize> {
    recover_stale_automation_runs(&state).await?;
    let now = gateway_now_ms();
    let due = state
        .inner
        .durability
        .due_automation_tasks(now, AUTOMATION_DUE_LIMIT)
        .await?;
    let mut accepted = 0;
    for task in due {
        let schedule = automation_schedule_from_value(task.schedule.clone())?;
        if latest_due_at_ms(&schedule, task.created_at_ms, task.last_run_at_ms, now)?.is_none() {
            continue;
        }
        if start_automation_run(state.clone(), task, "scheduler", None)
            .await?
            .is_some()
        {
            accepted += 1;
        }
    }
    Ok(accepted)
}

pub(super) async fn recover_stale_automation_runs(state: &WebState) -> psychevo::Result<usize> {
    let now = gateway_now_ms();
    let candidates = state
        .inner
        .durability
        .stale_automation_runs_for_recovery(
            now,
            AUTOMATION_STALE_RUN_RECOVERY_MS,
            AUTOMATION_STALE_RUN_RECOVERY_LIMIT,
        )
        .await?;
    let mut recovered = 0;
    for candidate in candidates {
        let next = next_run_after_now(&candidate.task)?;
        let thread_id = candidate
            .run
            .thread_id
            .as_deref()
            .or(candidate.task.target_thread_id.as_deref());
        let source_key = candidate
            .run
            .source_key
            .as_deref()
            .or(candidate.task.source_key.as_deref());
        let metadata = json!({
            "trigger": candidate.run.trigger.clone(),
            "recoveredAtMs": now,
        });
        if state
            .inner
            .durability
            .finish_automation_run(AutomationRunFinishInput {
                run_id: &candidate.run.id,
                status: AutomationRunTerminalStatus::Failed,
                thread_id,
                source_key,
                error: Some(AUTOMATION_STALE_RUN_RECOVERY_ERROR),
                metadata: Some(metadata),
                next_run_at_ms: next,
            })
            .await?
            .is_some()
        {
            recovered += 1;
        }
    }
    Ok(recovered)
}

pub(super) async fn start_automation_run(
    state: WebState,
    task: AutomationTaskRecord,
    trigger: &str,
    out_tx: Option<ConnectionSender>,
) -> psychevo::Result<Option<AutomationRunRecord>> {
    let gateway = state.inner.gateway.clone();
    let permit = gateway
        .acquire_activity_permit()
        .map_err(|error| psychevo::Error::Message(error.to_string()))?;
    let Some(run) = state
        .inner
        .durability
        .claim_automation_run(&task.id, trigger)
        .await?
    else {
        return Ok(None);
    };
    let run_for_task = run.clone();
    gateway.spawn_permitted_activity(format!("automation-run:{}", run.id), permit, async move {
        execute_automation_run(state, task, run_for_task, out_tx).await;
    });
    Ok(Some(run))
}

async fn execute_automation_run(
    state: WebState,
    task: AutomationTaskRecord,
    run: AutomationRunRecord,
    out_tx: Option<ConnectionSender>,
) {
    let result = send_automation_turn(&state, &task, out_tx.clone()).await;
    match result {
        Ok(turn_result) => {
            let next = next_run_after_now(&task).unwrap_or(None);
            let thread_id = turn_result.result.thread_id.clone();
            let (status, outcome) = match turn_result.result.outcome {
                psychevo::TurnOutcome::Completed => {
                    (AutomationRunTerminalStatus::Completed, "normal")
                }
                psychevo::TurnOutcome::Failed => (AutomationRunTerminalStatus::Failed, "failed"),
                psychevo::TurnOutcome::Stopped => {
                    (AutomationRunTerminalStatus::Interrupted, "stopped")
                }
                psychevo::TurnOutcome::Interrupted => {
                    (AutomationRunTerminalStatus::Interrupted, "aborted")
                }
            };
            let error = match (status, turn_result.result.terminal_error.as_ref()) {
                (AutomationRunTerminalStatus::Failed, Some(error)) => Some(error.message.as_str()),
                _ => None,
            };
            let source_key = turn_result
                .thread
                .source_key
                .as_ref()
                .map(|key| key.0.as_str());
            let metadata = json!({
                "turnId": turn_result.turn.id,
                "outcome": outcome,
                "trigger": run.trigger,
            });
            let _ = state
                .inner
                .durability
                .finish_automation_run(AutomationRunFinishInput {
                    run_id: &run.id,
                    status,
                    thread_id: Some(&thread_id),
                    source_key,
                    error,
                    metadata: Some(metadata),
                    next_run_at_ms: next,
                })
                .await;
        }
        Err(err) => {
            let next = next_run_after_now(&task).unwrap_or(None);
            let error = err.to_string();
            let _ = state
                .inner
                .durability
                .finish_automation_run(AutomationRunFinishInput {
                    run_id: &run.id,
                    status: AutomationRunTerminalStatus::Failed,
                    thread_id: task.target_thread_id.as_deref(),
                    source_key: task.source_key.as_deref(),
                    error: Some(&error),
                    metadata: Some(json!({"trigger": run.trigger})),
                    next_run_at_ms: next,
                })
                .await;
        }
    }
}

async fn send_automation_turn(
    state: &WebState,
    task: &AutomationTaskRecord,
    out_tx: Option<ConnectionSender>,
) -> psychevo::Result<GatewayTurnResult> {
    let cwd = PathBuf::from(&task.cwd);
    let (mut thread_id, source) = match automation_kind_to_wire(task.kind) {
        wire::automations::AutomationTaskKind::Project => {
            let source = automation_source(&task.id, &task.title);
            let thread_id = state.inner.gateway.resolve_source_thread(&source).await?;
            (thread_id, Some(source))
        }
        wire::automations::AutomationTaskKind::ThreadHeartbeat => {
            let thread_id = task.target_thread_id.clone().ok_or_else(|| {
                Error::Message("thread heartbeat automation requires a target thread".to_string())
            })?;
            (Some(thread_id), None)
        }
    };
    if thread_id.is_none() {
        let mut start = psychevo::StartThreadRequest::new(&cwd);
        start.source = "automation".to_string();
        start.metadata = Some(json!({"automationId": task.id}));
        let thread = state.inner.framework.start_thread(start).await?;
        thread_id = Some(thread.id().to_string());
        if let Some(source) = source.as_ref() {
            state
                .inner
                .gateway
                .bind_source_thread(
                    source,
                    thread.id(),
                    &GatewayBackendInfo {
                        kind: BackendKind::Native,
                        runtime_ref: None,
                        native_id: None,
                    },
                    Some(json!({"automationId": task.id})),
                )
                .await?;
        }
    }
    let event_selector = thread_id
        .as_ref()
        .map(GatewayThreadSelector::thread_id)
        .or_else(|| {
            source
                .as_ref()
                .map(|source| GatewayThreadSelector::source(source.source_key()))
        });
    let event_thread_id = thread_id.clone();
    let event_state = state.clone();
    let event_cwd = cwd.clone();
    let event_tx = out_tx.clone();
    let (mut caller, mut intent) = state.thread_turn_request(
        cwd,
        thread_id,
        vec![GatewayInputPart::Text {
            text: task.prompt.clone(),
        }],
    );
    intent.source = source;
    intent.policy.model = task.model.clone();
    intent.policy.reasoning_effort = task.reasoning_effort.clone();
    caller.set_runtime_tools(Vec::new());
    match automation_execution_from_value(task.execution.clone())?.policy {
        wire::automations::AutomationExecutionPolicy::AutoSandbox => {
            intent.policy.permission_mode = Some(PermissionMode::BypassPermissions);
            intent.policy.sandbox_override = Some(RunSandboxOverride::workspace_write());
        }
        wire::automations::AutomationExecutionPolicy::AskFirst => {
            intent.policy.permission_mode = Some(PermissionMode::Default);
        }
    }
    caller.surface = ThreadSurface::Automation;
    caller.runtime_source = "automation".to_string();
    caller.continue_sources = vec![
        "run".to_string(),
        "tui".to_string(),
        "web".to_string(),
        "automation".to_string(),
    ];
    intent.turn_id = Some(Uuid::now_v7().to_string());
    caller.observe_gateway_events(move |event| {
        let context = event_selector
            .as_ref()
            .map(|selector| {
                event_state.pending_context_for_selector(selector, event_thread_id.as_deref())
            })
            .unwrap_or_default();
        event_state.publish_gateway_event_for_connection(
            event,
            context,
            Some(&event_cwd),
            event_tx.as_ref(),
        );
    });
    let submission = intent.into_framework_request(caller)?;
    let observers = submission.observers;
    let thread = state
        .inner
        .framework
        .resume_thread(&submission.thread_id)
        .await?;
    let handle = thread.start_turn(submission.request).await?;
    observers.attach(&state.inner.gateway, handle.clone());
    let receipt = handle.receipt().clone();
    let result = handle.wait().await?;
    Ok(framework_gateway_turn_result(receipt, result))
}
