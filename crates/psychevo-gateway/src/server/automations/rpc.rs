use std::path::Path;

use psychevo::Error;
use psychevo::application::AutomationTaskInput;
use psychevo::automations::next_run_at_ms;
use psychevo::model_state::normalize_reasoning_effort;
use psychevo::paths::canonicalize_cwd;
use psychevo::{PermissionMode, RunMode, RunSandboxOverride};
use psychevo_gateway_protocol as wire;
use serde_json::Value;
use uuid::Uuid;

use super::draft::{automation_draft_prompt, parse_automation_draft_response};
use super::runner::{recover_stale_automation_runs, start_automation_run};
use super::support::{
    automation_kind_from_wire, automation_run_view, automation_schedule_from_value,
    automation_source, automation_task_for_request, automation_task_view, normalize_optional,
    resolve_automation_target_scope,
};
use crate::gateway_now_ms;
use crate::server::auth_input::authorize_thread;
use crate::server::binding::{AuthContext, WebState};
use crate::server::event_delivery::ConnectionSender;
use crate::server::scope_session::{
    resolve_optional_scope, resolve_required_scope, resolved_scope_for_thread,
};

pub(in crate::server) async fn automation_list_result(
    state: &WebState,
    _auth: &AuthContext,
    params: wire::automations::AutomationListParams,
) -> psychevo::Result<Value> {
    let store = &state.inner.durability;
    let records = match params.cwd {
        Some(cwd) => {
            let cwd = canonicalize_cwd(Path::new(&cwd))?;
            store
                .automation_tasks_for_cwd(&cwd.display().to_string())
                .await?
        }
        None => store.automation_tasks_for_optional_cwd(None).await?,
    };
    let mut automations = Vec::with_capacity(records.len());
    for record in records {
        automations.push(automation_task_view(state, record).await?);
    }
    Ok(serde_json::to_value(
        wire::automations::AutomationListResult { automations },
    )?)
}

pub(in crate::server) async fn automation_draft_result(
    state: WebState,
    auth: &AuthContext,
    params: wire::automations::AutomationDraftParams,
) -> psychevo::Result<Value> {
    let request = params.request.trim().to_string();
    if request.is_empty() {
        return Err(Error::Message(
            "automation draft request is required".to_string(),
        ));
    }
    if request.chars().count() > 4_000 {
        return Err(Error::Message(
            "automation draft request is too long".to_string(),
        ));
    }

    let current_thread_id = normalize_optional(params.current_thread_id);
    let (cwd, current_thread_id) = match current_thread_id {
        Some(thread_id) => {
            authorize_thread(&state, auth, &thread_id).await?;
            let thread_scope = resolved_scope_for_thread(&state, &thread_id).await?;
            if let Some(scope) = params.scope {
                let scope = resolve_required_scope(&state, auth, scope)?;
                if scope.cwd != thread_scope.cwd {
                    return Err(Error::Message(
                        "automation draft scope must match current thread cwd".to_string(),
                    ));
                }
            }
            (thread_scope.cwd, Some(thread_id))
        }
        None => {
            let scope = resolve_optional_scope(&state, auth, params.scope)?;
            (scope.cwd, None)
        }
    };

    let prompt = automation_draft_prompt(
        &request,
        &cwd.display().to_string(),
        current_thread_id.as_deref(),
    );
    let mut inherited_env = state.inner.inherited_env.clone();
    inherited_env
        .entry("PSYCHEVO_HOME".to_string())
        .or_insert_with(|| state.inner.home.to_string_lossy().into_owned());
    let turn = psychevo::TurnRequest::new(prompt)
        .with_identity("automation-draft", None)
        .with_execution_policy(
            RunMode::Default,
            Some(PermissionMode::Default),
            state.inner.config_path.clone(),
        )
        .with_approval(None, false)
        .with_environment(
            Some(inherited_env),
            None,
            Some(RunSandboxOverride::read_only()),
        )
        .with_agent(None, true, true)
        .with_runtime_tools(Vec::new())
        .with_framework_context(
            Some(state.inner.home.join("snapshots")),
            None,
            Vec::new(),
            None,
        );
    let mut start = psychevo::StartThreadRequest::new(&cwd);
    start.source = "automation-draft".to_string();
    let result = state
        .inner
        .framework
        .start_thread_with_turn(start, turn)
        .await?
        .wait()
        .await?;
    let draft =
        parse_automation_draft_response(&result.final_answer, current_thread_id.as_deref())?;
    Ok(serde_json::to_value(
        wire::automations::AutomationDraftResult { draft },
    )?)
}

pub(in crate::server) async fn automation_write_result(
    state: &WebState,
    auth: &AuthContext,
    params: wire::automations::AutomationWriteParams,
) -> psychevo::Result<Value> {
    let automation_id = params
        .automation_id
        .clone()
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| Uuid::now_v7().to_string());
    let existing = state
        .inner
        .durability
        .automation_task(&automation_id)
        .await?;
    let target = resolve_automation_target_scope(state, auth, params.scope, &params.target).await?;
    let cwd = target.cwd.display().to_string();
    if let Some(existing) = existing.as_ref()
        && existing.cwd != cwd
    {
        return Err(Error::Message(
            "automation cwd cannot change after creation".to_string(),
        ));
    }

    let title = params.title.trim().to_string();
    if title.is_empty() {
        return Err(Error::Message("automation title is required".to_string()));
    }
    let prompt = params.prompt.trim().to_string();
    if prompt.is_empty() {
        return Err(Error::Message("automation prompt is required".to_string()));
    }
    let schedule_value = serde_json::to_value(&params.schedule)?;
    let schedule = automation_schedule_from_value(schedule_value.clone())?;
    let execution = params.execution.unwrap_or_default();
    let execution_value = serde_json::to_value(&execution)?;
    let enabled = existing.as_ref().is_none_or(|record| record.enabled);
    let now = gateway_now_ms();
    let created_at_ms = existing
        .as_ref()
        .map(|record| record.created_at_ms)
        .unwrap_or(now);
    let last_run_at_ms = existing.as_ref().and_then(|record| record.last_run_at_ms);
    let next_run_at_ms = if enabled {
        next_run_at_ms(&schedule, created_at_ms, last_run_at_ms, now)?
    } else {
        None
    };
    let source_key = match target.kind {
        wire::automations::AutomationTaskKind::Project => {
            Some(automation_source(&automation_id, &title).source_key().0)
        }
        wire::automations::AutomationTaskKind::ThreadHeartbeat => None,
    };
    let record = state
        .inner
        .durability
        .upsert_automation_task(AutomationTaskInput {
            id: Some(automation_id),
            cwd,
            kind: automation_kind_from_wire(target.kind),
            target_thread_id: target.target_thread_id,
            title,
            prompt,
            schedule: schedule_value,
            enabled,
            execution: execution_value,
            model: normalize_optional(params.model),
            reasoning_effort: normalize_reasoning_effort(params.reasoning_effort),
            source_key,
            next_run_at_ms,
        })
        .await?;
    Ok(serde_json::to_value(
        wire::automations::AutomationMutationResult {
            automation: automation_task_view(state, record).await?,
        },
    )?)
}

pub(in crate::server) async fn automation_set_enabled_result(
    state: &WebState,
    auth: &AuthContext,
    params: wire::automations::AutomationIdParams,
    enabled: bool,
) -> psychevo::Result<Value> {
    let existing = automation_task_for_request(state, auth, &params.automation_id).await?;
    let schedule = automation_schedule_from_value(existing.schedule.clone())?;
    let next_run_at_ms = if enabled {
        next_run_at_ms(
            &schedule,
            existing.created_at_ms,
            existing.last_run_at_ms,
            gateway_now_ms(),
        )?
    } else {
        None
    };
    let record = state
        .inner
        .durability
        .upsert_automation_task(AutomationTaskInput {
            id: Some(existing.id),
            cwd: existing.cwd,
            kind: existing.kind,
            target_thread_id: existing.target_thread_id,
            title: existing.title,
            prompt: existing.prompt,
            schedule: existing.schedule,
            enabled,
            execution: existing.execution,
            model: existing.model,
            reasoning_effort: existing.reasoning_effort,
            source_key: existing.source_key,
            next_run_at_ms,
        })
        .await?;
    Ok(serde_json::to_value(
        wire::automations::AutomationMutationResult {
            automation: automation_task_view(state, record).await?,
        },
    )?)
}

pub(in crate::server) async fn automation_delete_result(
    state: &WebState,
    auth: &AuthContext,
    params: wire::automations::AutomationIdParams,
) -> psychevo::Result<Value> {
    let _record = automation_task_for_request(state, auth, &params.automation_id).await?;
    let deleted = state
        .inner
        .durability
        .delete_automation_task(&params.automation_id)
        .await?;
    Ok(serde_json::to_value(
        wire::automations::AutomationDeleteResult {
            deleted,
            automation_id: params.automation_id,
        },
    )?)
}

pub(in crate::server) async fn automation_run_result(
    state: WebState,
    auth: &AuthContext,
    params: wire::automations::AutomationRunParams,
    out_tx: ConnectionSender,
) -> psychevo::Result<Value> {
    recover_stale_automation_runs(&state).await?;
    let task = automation_task_for_request(&state, auth, &params.automation_id).await?;
    let trigger = params
        .trigger
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("manual");
    let run = start_automation_run(state.clone(), task.clone(), trigger, Some(out_tx)).await?;
    let automation = state
        .inner
        .durability
        .automation_task(&task.id)
        .await?
        .unwrap_or(task);
    Ok(serde_json::to_value(
        wire::automations::AutomationRunResult {
            accepted: run.is_some(),
            automation: automation_task_view(&state, automation).await?,
            run: run.map(automation_run_view),
        },
    )?)
}
