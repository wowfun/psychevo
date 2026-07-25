pub(super) async fn run_due_automations_once(state: WebState) -> psychevo_runtime::Result<usize> {
    recover_stale_automation_runs(&state).await?;
    let now = gateway_now_ms();
    let due = state
        .inner
        .state

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

async fn recover_stale_automation_runs(state: &WebState) -> psychevo_runtime::Result<usize> {
    let now = gateway_now_ms();
    let candidates = state
        .inner
        .state

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
            .state

            .finish_automation_run(AutomationRunFinishInput {
                run_id: &candidate.run.id,
                status: "failed",
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

async fn start_automation_run(
    state: WebState,
    task: AutomationTaskRecord,
    trigger: &str,
    out_tx: Option<ConnectionSender>,
) -> psychevo_runtime::Result<Option<AutomationRunRecord>> {
    let Some(run) = state
        .inner
        .state

        .claim_automation_run(&task.id, trigger)
        .await?
    else {
        return Ok(None);
    };
    let run_for_task = run.clone();
    tokio::spawn(async move {
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
            let thread_id = turn_result.result.session_id.clone();
            let source_key = turn_result
                .thread
                .source_key
                .as_ref()
                .map(|key| key.0.as_str());
            let metadata = json!({
                "turnId": turn_result.turn.id,
                "outcome": turn_result.result.outcome.as_str(),
                "trigger": run.trigger,
            });
            let _ = state
                .inner
                .state

                .finish_automation_run(AutomationRunFinishInput {
                    run_id: &run.id,
                    status: "completed",
                    thread_id: Some(&thread_id),
                    source_key,
                    error: None,
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
                .state

                .finish_automation_run(AutomationRunFinishInput {
                    run_id: &run.id,
                    status: "failed",
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
) -> psychevo_runtime::Result<GatewayTurnResult> {
    let cwd = PathBuf::from(&task.cwd);
    let (thread_id, source, bind_source) = match automation_kind_from_str(&task.kind)? {
        wire::AutomationTaskKind::Project => {
            let source = automation_source(&task.id, &task.title);
            let thread_id = state.inner.gateway.resolve_source_thread(&source).await?;
            (thread_id, Some(source.clone()), Some(source))
        }
        wire::AutomationTaskKind::ThreadHeartbeat => {
            let thread_id = task.target_thread_id.clone().ok_or_else(|| {
                Error::Message("thread heartbeat automation requires a target thread".to_string())
            })?;
            (Some(thread_id), None, None)
        }
    };
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
    intent.bind_source = bind_source;
    intent.policy.model = task.model.clone();
    intent.policy.reasoning_effort = task.reasoning_effort.clone();
    caller.set_runtime_tools(Vec::new());
    match automation_execution_from_value(task.execution.clone())?.policy {
        wire::AutomationExecutionPolicy::AutoSandbox => {
            intent.policy.permission_mode = Some(PermissionMode::BypassPermissions);
            intent.policy.sandbox_override = Some(RunSandboxOverride::workspace_write());
        }
        wire::AutomationExecutionPolicy::AskFirst => {
            intent.policy.permission_mode = Some(PermissionMode::Default);
        }
    }
    caller.surface = crate::ThreadSurface::Automation;
    caller.runtime_source = "automation".to_string();
    caller.continue_sources = vec![
        "run".to_string(),
        "tui".to_string(),
        "web".to_string(),
        "automation".to_string(),
    ];
    intent.lineage = Some(json!({"automationId": task.id}));
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
    state
        .inner
        .gateway
        .start_turn(caller, intent)
        .await?
        .wait()
        .await
}
