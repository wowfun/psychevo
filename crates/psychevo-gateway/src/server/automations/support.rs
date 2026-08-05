use std::path::PathBuf;

use psychevo::Error;
use psychevo::application::{AutomationRunRecord, AutomationTaskKind, AutomationTaskRecord};
use psychevo::automations::{AutomationSchedule, next_run_at_ms};
use psychevo_gateway_protocol as wire;
use serde_json::{Value, json};

use crate::gateway_now_ms;
use crate::server::auth_input::authorize_thread;
use crate::server::binding::{AuthContext, WebState};
use crate::server::scope_session::{
    resolve_optional_scope, resolve_required_scope, resolved_scope_for_thread,
};
use psychevo_gateway_protocol::source::GatewaySource;

const AUTOMATION_RUN_HISTORY_LIMIT: usize = 5;

pub(super) async fn resolve_automation_target_scope(
    state: &WebState,
    auth: &AuthContext,
    scope: Option<wire::source::GatewayRequestScope>,
    target: &wire::automations::AutomationTargetInput,
) -> psychevo::Result<ResolvedAutomationTarget> {
    match target {
        wire::automations::AutomationTargetInput::Project => {
            let scope = resolve_optional_scope(state, auth, scope)?;
            Ok(ResolvedAutomationTarget {
                cwd: scope.cwd,
                kind: wire::automations::AutomationTaskKind::Project,
                target_thread_id: None,
            })
        }
        wire::automations::AutomationTargetInput::ThreadHeartbeat { thread_id } => {
            authorize_thread(state, auth, thread_id).await?;
            let thread_scope = resolved_scope_for_thread(state, thread_id).await?;
            if let Some(scope) = scope {
                let scope = resolve_required_scope(state, auth, scope)?;
                if scope.cwd != thread_scope.cwd {
                    return Err(Error::Message(
                        "automation scope must match target thread cwd".to_string(),
                    ));
                }
            }
            Ok(ResolvedAutomationTarget {
                cwd: thread_scope.cwd,
                kind: wire::automations::AutomationTaskKind::ThreadHeartbeat,
                target_thread_id: Some(thread_id.clone()),
            })
        }
    }
}

pub(super) struct ResolvedAutomationTarget {
    pub(super) cwd: PathBuf,
    pub(super) kind: wire::automations::AutomationTaskKind,
    pub(super) target_thread_id: Option<String>,
}

pub(super) async fn automation_task_for_request(
    state: &WebState,
    _auth: &AuthContext,
    automation_id: &str,
) -> psychevo::Result<AutomationTaskRecord> {
    let record = state
        .inner
        .durability
        .automation_task(automation_id)
        .await?
        .ok_or_else(|| Error::Message(format!("automation not found: {automation_id}")))?;
    Ok(record)
}

pub(super) async fn automation_task_view(
    state: &WebState,
    record: AutomationTaskRecord,
) -> psychevo::Result<wire::automations::AutomationTaskView> {
    let runs = state
        .inner
        .durability
        .automation_runs_for_task(&record.id, AUTOMATION_RUN_HISTORY_LIMIT)
        .await?
        .into_iter()
        .map(automation_run_view)
        .collect();
    Ok(wire::automations::AutomationTaskView {
        id: record.id,
        cwd: record.cwd,
        kind: automation_kind_to_wire(record.kind),
        target_thread_id: record.target_thread_id,
        title: record.title,
        prompt: record.prompt,
        schedule: serde_json::from_value(record.schedule)?,
        enabled: record.enabled,
        execution: automation_execution_from_value(record.execution)?,
        model: record.model,
        reasoning_effort: record.reasoning_effort,
        source_key: record.source_key,
        created_at_ms: record.created_at_ms,
        updated_at_ms: record.updated_at_ms,
        last_run_at_ms: record.last_run_at_ms,
        next_run_at_ms: record.next_run_at_ms,
        last_status: record.last_status.map(|status| status.as_str().to_string()),
        last_error: record.last_error,
        runs,
    })
}

pub(super) fn automation_run_view(
    record: AutomationRunRecord,
) -> wire::automations::AutomationRunView {
    wire::automations::AutomationRunView {
        id: record.id,
        automation_id: record.automation_id,
        trigger: record.trigger,
        status: record.status.as_str().to_string(),
        started_at_ms: record.started_at_ms,
        completed_at_ms: record.completed_at_ms,
        thread_id: record.thread_id,
        source_key: record.source_key,
        error: record.error,
        metadata: record.metadata,
    }
}

pub(super) fn automation_source(id: &str, title: &str) -> GatewaySource {
    GatewaySource::new("automation", id)
        .persistent()
        .with_visible_name(title.to_string())
        .with_raw_identity(json!({"kind": "automation", "automationId": id}))
}

pub(super) fn next_run_after_now(task: &AutomationTaskRecord) -> psychevo::Result<Option<i64>> {
    if !task.enabled {
        return Ok(None);
    }
    let schedule = automation_schedule_from_value(task.schedule.clone())?;
    let now = gateway_now_ms();
    next_run_at_ms(&schedule, task.created_at_ms, Some(now), now)
}

pub(super) fn automation_schedule_from_value(value: Value) -> psychevo::Result<AutomationSchedule> {
    serde_json::from_value(value).map_err(Into::into)
}

pub(super) fn automation_execution_from_value(
    value: Value,
) -> psychevo::Result<wire::automations::AutomationExecutionInput> {
    serde_json::from_value(value).map_err(Into::into)
}

pub(super) fn automation_kind_to_wire(
    kind: AutomationTaskKind,
) -> wire::automations::AutomationTaskKind {
    match kind {
        AutomationTaskKind::Project => wire::automations::AutomationTaskKind::Project,
        AutomationTaskKind::ThreadHeartbeat => {
            wire::automations::AutomationTaskKind::ThreadHeartbeat
        }
    }
}

pub(super) fn automation_kind_from_wire(
    kind: wire::automations::AutomationTaskKind,
) -> AutomationTaskKind {
    match kind {
        wire::automations::AutomationTaskKind::Project => AutomationTaskKind::Project,
        wire::automations::AutomationTaskKind::ThreadHeartbeat => {
            AutomationTaskKind::ThreadHeartbeat
        }
    }
}

pub(super) fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
