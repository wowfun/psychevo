fn resolve_automation_target_scope(
    state: &WebState,
    auth: &AuthContext,
    scope: Option<wire::GatewayRequestScope>,
    target: &wire::AutomationTargetInput,
) -> psychevo_runtime::Result<ResolvedAutomationTarget> {
    match target {
        wire::AutomationTargetInput::Project => {
            let scope = resolve_optional_scope(state, auth, scope)?;
            Ok(ResolvedAutomationTarget {
                cwd: scope.cwd,
                kind: wire::AutomationTaskKind::Project,
                target_thread_id: None,
            })
        }
        wire::AutomationTargetInput::ThreadHeartbeat { thread_id } => {
            authorize_thread(state, auth, thread_id)?;
            let thread_scope = resolved_scope_for_thread(state, thread_id)?;
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
                kind: wire::AutomationTaskKind::ThreadHeartbeat,
                target_thread_id: Some(thread_id.clone()),
            })
        }
    }
}

struct ResolvedAutomationTarget {
    cwd: PathBuf,
    kind: wire::AutomationTaskKind,
    target_thread_id: Option<String>,
}

fn automation_task_for_request(
    state: &WebState,
    _auth: &AuthContext,
    automation_id: &str,
) -> psychevo_runtime::Result<AutomationTaskRecord> {
    let record = state
        .inner
        .state

        .automation_task(automation_id)?
        .ok_or_else(|| Error::Message(format!("automation not found: {automation_id}")))?;
    Ok(record)
}

fn automation_task_view(
    state: &WebState,
    record: AutomationTaskRecord,
) -> psychevo_runtime::Result<wire::AutomationTaskView> {
    let runs = state
        .inner
        .state

        .automation_runs_for_task(&record.id, AUTOMATION_RUN_HISTORY_LIMIT)?
        .into_iter()
        .map(automation_run_view)
        .collect();
    Ok(wire::AutomationTaskView {
        id: record.id,
        cwd: record.cwd,
        kind: automation_kind_from_str(&record.kind)?,
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
        last_status: record.last_status,
        last_error: record.last_error,
        runs,
    })
}

fn automation_run_view(record: AutomationRunRecord) -> wire::AutomationRunView {
    wire::AutomationRunView {
        id: record.id,
        automation_id: record.automation_id,
        trigger: record.trigger,
        status: record.status,
        started_at_ms: record.started_at_ms,
        completed_at_ms: record.completed_at_ms,
        thread_id: record.thread_id,
        source_key: record.source_key,
        error: record.error,
        metadata: record.metadata,
    }
}

fn automation_source(id: &str, title: &str) -> GatewaySource {
    GatewaySource::new("automation", id)
        .persistent()
        .with_visible_name(title.to_string())
        .with_raw_identity(json!({"kind": "automation", "automationId": id}))
}

fn next_run_after_now(task: &AutomationTaskRecord) -> psychevo_runtime::Result<Option<i64>> {
    if !task.enabled {
        return Ok(None);
    }
    let schedule = automation_schedule_from_value(task.schedule.clone())?;
    let now = gateway_now_ms();
    next_run_at_ms(&schedule, task.created_at_ms, Some(now), now)
}

fn automation_schedule_from_value(value: Value) -> psychevo_runtime::Result<AutomationSchedule> {
    serde_json::from_value(value).map_err(Into::into)
}

fn automation_execution_from_value(
    value: Value,
) -> psychevo_runtime::Result<wire::AutomationExecutionInput> {
    serde_json::from_value(value).map_err(Into::into)
}

fn automation_kind_from_str(value: &str) -> psychevo_runtime::Result<wire::AutomationTaskKind> {
    match value {
        "project" => Ok(wire::AutomationTaskKind::Project),
        "threadHeartbeat" => Ok(wire::AutomationTaskKind::ThreadHeartbeat),
        _ => Err(Error::Message(format!("unknown automation kind: {value}"))),
    }
}

fn automation_kind_str(kind: wire::AutomationTaskKind) -> &'static str {
    match kind {
        wire::AutomationTaskKind::Project => "project",
        wire::AutomationTaskKind::ThreadHeartbeat => "threadHeartbeat",
    }
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
