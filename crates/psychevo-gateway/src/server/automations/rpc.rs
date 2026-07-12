pub(super) fn automation_list_result(
    state: &WebState,
    _auth: &AuthContext,
    params: wire::AutomationListParams,
) -> psychevo_runtime::Result<Value> {
    let store = state.inner.state.store();
    let records = match params.cwd {
        Some(cwd) => {
            let cwd = canonicalize_cwd(Path::new(&cwd))?;
            store.automation_tasks_for_cwd(&cwd.display().to_string())?
        }
        None => store.automation_tasks_for_optional_cwd(None)?,
    };
    let automations = records
        .into_iter()
        .map(|record| automation_task_view(state, record))
        .collect::<psychevo_runtime::Result<Vec<_>>>()?;
    Ok(serde_json::to_value(wire::AutomationListResult {
        automations,
    })?)
}

pub(super) async fn automation_draft_result(
    state: WebState,
    auth: &AuthContext,
    params: wire::AutomationDraftParams,
) -> psychevo_runtime::Result<Value> {
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
            authorize_thread(&state, auth, &thread_id)?;
            let thread_scope = resolved_scope_for_thread(&state, &thread_id)?;
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

    let mut options = state.run_options(cwd.clone(), None);
    options.prompt = automation_draft_prompt(
        &request,
        &cwd.display().to_string(),
        current_thread_id.as_deref(),
    );
    options.no_agents = true;
    options.no_skills = true;
    options.clarify_enabled = false;
    options.permission_mode = Some(PermissionMode::Default);
    options.sandbox_override = Some(RunSandboxOverride::read_only());
    options.runtime_tools.clear();

    let profile = crate::generated_gateway_runtime_profiles()
        .into_iter()
        .find(|profile| profile.runtime == RuntimeProfileKind::Native)
        .expect("generated Native Agent profile");
    let result = state
        .inner
        .gateway
        .run_internal_agent_turn(
            None,
            profile,
            None,
            crate::BackendTurnRequest {
                options,
                input: Vec::new(),
                runtime_source: "automation-draft".to_string(),
                continue_sources: vec!["automation-draft".to_string()],
                stream: None,
                control: None,
            },
            Uuid::now_v7().to_string(),
            None,
        )
        .await?;
    let draft =
        parse_automation_draft_response(&result.final_answer, current_thread_id.as_deref())?;
    Ok(serde_json::to_value(wire::AutomationDraftResult { draft })?)
}

pub(super) fn automation_write_result(
    state: &WebState,
    auth: &AuthContext,
    params: wire::AutomationWriteParams,
) -> psychevo_runtime::Result<Value> {
    let automation_id = params
        .automation_id
        .clone()
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| Uuid::now_v7().to_string());
    let existing = state.inner.state.store().automation_task(&automation_id)?;
    let target = resolve_automation_target_scope(state, auth, params.scope, &params.target)?;
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
        wire::AutomationTaskKind::Project => {
            Some(automation_source(&automation_id, &title).source_key().0)
        }
        wire::AutomationTaskKind::ThreadHeartbeat => None,
    };
    let record = state
        .inner
        .state
        .store()
        .upsert_automation_task(AutomationTaskInput {
            id: Some(automation_id),
            cwd,
            kind: automation_kind_str(target.kind).to_string(),
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
        })?;
    Ok(serde_json::to_value(wire::AutomationMutationResult {
        automation: automation_task_view(state, record)?,
    })?)
}

pub(super) fn automation_set_enabled_result(
    state: &WebState,
    auth: &AuthContext,
    params: wire::AutomationIdParams,
    enabled: bool,
) -> psychevo_runtime::Result<Value> {
    let existing = automation_task_for_request(state, auth, &params.automation_id)?;
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
        .state
        .store()
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
        })?;
    Ok(serde_json::to_value(wire::AutomationMutationResult {
        automation: automation_task_view(state, record)?,
    })?)
}

pub(super) fn automation_delete_result(
    state: &WebState,
    auth: &AuthContext,
    params: wire::AutomationIdParams,
) -> psychevo_runtime::Result<Value> {
    let _record = automation_task_for_request(state, auth, &params.automation_id)?;
    let deleted = state
        .inner
        .state
        .store()
        .delete_automation_task(&params.automation_id)?;
    Ok(serde_json::to_value(wire::AutomationDeleteResult {
        deleted,
        automation_id: params.automation_id,
    })?)
}

pub(super) fn automation_run_result(
    state: WebState,
    auth: &AuthContext,
    params: wire::AutomationRunParams,
    out_tx: mpsc::UnboundedSender<String>,
) -> psychevo_runtime::Result<Value> {
    recover_stale_automation_runs(&state)?;
    let task = automation_task_for_request(&state, auth, &params.automation_id)?;
    let trigger = params
        .trigger
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("manual");
    let run = start_automation_run(state.clone(), task.clone(), trigger, Some(out_tx))?;
    let automation = state
        .inner
        .state
        .store()
        .automation_task(&task.id)?
        .unwrap_or(task);
    Ok(serde_json::to_value(wire::AutomationRunResult {
        accepted: run.is_some(),
        automation: automation_task_view(&state, automation)?,
        run: run.map(automation_run_view),
    })?)
}
