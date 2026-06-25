use super::*;
use futures::future::BoxFuture;
use psychevo_agent_core::{ToolBinding, ToolExecutionMode, ToolOutput};
use psychevo_ai::AbortSignal;

const AUTOMATION_RUN_HISTORY_LIMIT: usize = 5;
const AUTOMATION_DUE_LIMIT: usize = 10;
const AUTOMATION_SCHEDULER_TICK_MS: u64 = 30_000;

pub(super) fn reconcile(state: WebState) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    let _handle = tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(AUTOMATION_SCHEDULER_TICK_MS));
        loop {
            tick.tick().await;
            if let Err(err) = run_due_automations_once(state.clone()).await {
                eprintln!("automation scheduler failed: {err}");
            }
        }
    });
}

pub(super) fn automation_runtime_tools(
    state: WebState,
    workdir: PathBuf,
    current_thread_id: Option<String>,
) -> Vec<psychevo_runtime::RuntimeTool> {
    vec![psychevo_runtime::RuntimeTool::new(Arc::new(
        AutomationTool {
            state,
            workdir,
            current_thread_id,
        },
    ))]
}

#[derive(Clone)]
struct AutomationTool {
    state: WebState,
    workdir: PathBuf,
    current_thread_id: Option<String>,
}

impl ToolBinding for AutomationTool {
    fn name(&self) -> &str {
        "automation"
    }

    fn description(&self) -> &str {
        "Create, list, update, pause, resume, run, or remove Psychevo local automations. Use this when the user asks in natural language to schedule or manage recurring or one-shot work. If the user does not specify a target, create automations for the Gateway-supplied current thread when available; otherwise use the current project."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "create", "update", "pause", "resume", "run", "remove"]
                },
                "automationId": {
                    "type": "string",
                    "description": "Required for update, pause, resume, run, and remove"
                },
                "title": { "type": "string" },
                "prompt": { "type": "string" },
                "target": {
                    "description": "Omit target for the default. Use \"currentThread\" only when this turn has a Gateway-supplied current thread. Use \"project\" for the current project, or an object like {\"kind\":\"threadHeartbeat\",\"threadId\":\"...\"}.",
                    "oneOf": [
                        { "type": "string", "enum": ["project", "currentThread"] },
                        {
                            "type": "object",
                            "properties": {
                                "kind": { "type": "string", "enum": ["project", "threadHeartbeat"] },
                                "threadId": { "type": "string" }
                            }
                        }
                    ]
                },
                "threadId": {
                    "type": "string",
                    "description": "Shortcut for target {\"kind\":\"threadHeartbeat\",\"threadId\":\"...\"}"
                },
                "schedule": {
                    "type": "object",
                    "description": "One of interval {kind,everyMinutes}, delay {kind,afterMinutes}, once {kind,at}, daily {kind,time}, weekly {kind,weekdays,time}"
                },
                "execution": {
                    "type": "object",
                    "properties": {
                        "policy": { "type": "string", "enum": ["autoSandbox", "askFirst"] }
                    }
                }
            }
        })
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let tool = self.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            match tool.execute_automation_action(args) {
                Ok(value) => ToolOutput::ok(value),
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

impl AutomationTool {
    fn execute_automation_action(&self, args: Value) -> psychevo_runtime::Result<Value> {
        let action = tool_string(&args, "action")?;
        match action.as_str() {
            "list" => self.list(),
            "create" => self.create_or_update(args, None),
            "update" => {
                let automation_id = tool_string(&args, "automationId")?;
                self.create_or_update(args, Some(automation_id))
            }
            "pause" => self.set_enabled(args, false),
            "resume" => self.set_enabled(args, true),
            "run" => {
                let automation_id = tool_string(&args, "automationId")?;
                let (tx, _rx) = mpsc::unbounded_channel();
                let value = automation_run_result(
                    self.state.clone(),
                    &AuthContext::Bearer,
                    wire::AutomationRunParams {
                        automation_id,
                        trigger: Some("tool".to_string()),
                    },
                    tx,
                )?;
                Ok(tool_result(action, value))
            }
            "remove" => {
                let automation_id = tool_string(&args, "automationId")?;
                let value = automation_delete_result(
                    &self.state,
                    &AuthContext::Bearer,
                    wire::AutomationIdParams { automation_id },
                )?;
                Ok(tool_result(action, value))
            }
            other => Err(Error::Message(format!(
                "unknown automation action: {other}"
            ))),
        }
    }

    fn list(&self) -> psychevo_runtime::Result<Value> {
        let value = automation_list_result(
            &self.state,
            &AuthContext::Bearer,
            wire::AutomationListParams {
                scope: Some(self.scope()),
            },
        )?;
        Ok(tool_result("list", value))
    }

    fn create_or_update(
        &self,
        args: Value,
        automation_id: Option<String>,
    ) -> psychevo_runtime::Result<Value> {
        let existing = automation_id
            .as_deref()
            .map(|id| automation_task_for_request(&self.state, &AuthContext::Bearer, id))
            .transpose()?;
        let params = self.write_params_from_args(&args, automation_id, existing.as_ref())?;
        let value = automation_write_result(&self.state, &AuthContext::Bearer, params)?;
        Ok(tool_result(
            if existing.is_some() {
                "update"
            } else {
                "create"
            },
            value,
        ))
    }

    fn set_enabled(&self, args: Value, enabled: bool) -> psychevo_runtime::Result<Value> {
        let automation_id = tool_string(&args, "automationId")?;
        let value = automation_set_enabled_result(
            &self.state,
            &AuthContext::Bearer,
            wire::AutomationIdParams { automation_id },
            enabled,
        )?;
        Ok(tool_result(if enabled { "resume" } else { "pause" }, value))
    }

    fn write_params_from_args(
        &self,
        args: &Value,
        automation_id: Option<String>,
        existing: Option<&AutomationTaskRecord>,
    ) -> psychevo_runtime::Result<wire::AutomationWriteParams> {
        let title = optional_tool_string(args, "title")
            .or_else(|| existing.map(|record| record.title.clone()))
            .ok_or_else(|| Error::Message("automation title is required".to_string()))?;
        let prompt = optional_tool_string(args, "prompt")
            .or_else(|| existing.map(|record| record.prompt.clone()))
            .ok_or_else(|| Error::Message("automation prompt is required".to_string()))?;
        let schedule = match args.get("schedule") {
            Some(value) => serde_json::from_value(value.clone())?,
            None => existing
                .map(|record| serde_json::from_value(record.schedule.clone()))
                .transpose()?
                .ok_or_else(|| Error::Message("automation schedule is required".to_string()))?,
        };
        let target = if args.get("target").is_some() || args.get("threadId").is_some() {
            self.target_from_args(args)?
        } else if let Some(existing) = existing {
            match automation_kind_from_str(&existing.kind)? {
                wire::AutomationTaskKind::Project => wire::AutomationTargetInput::Project,
                wire::AutomationTaskKind::ThreadHeartbeat => {
                    wire::AutomationTargetInput::ThreadHeartbeat {
                        thread_id: existing.target_thread_id.clone().ok_or_else(|| {
                            Error::Message(
                                "thread automation is missing a target thread".to_string(),
                            )
                        })?,
                    }
                }
            }
        } else if let Some(thread_id) = self.current_thread_id.clone() {
            wire::AutomationTargetInput::ThreadHeartbeat { thread_id }
        } else {
            wire::AutomationTargetInput::Project
        };
        let execution = match args.get("execution") {
            Some(value) => Some(serde_json::from_value(value.clone())?),
            None => existing
                .map(|record| automation_execution_from_value(record.execution.clone()))
                .transpose()?,
        };
        Ok(wire::AutomationWriteParams {
            automation_id,
            scope: Some(self.scope()),
            target,
            title,
            prompt,
            schedule,
            execution,
            model: optional_tool_string(args, "model")
                .or_else(|| existing.and_then(|record| record.model.clone())),
            reasoning_effort: optional_tool_string(args, "reasoningEffort")
                .or_else(|| existing.and_then(|record| record.reasoning_effort.clone())),
        })
    }

    fn target_from_args(
        &self,
        args: &Value,
    ) -> psychevo_runtime::Result<wire::AutomationTargetInput> {
        if let Some(thread_id) = optional_tool_string(args, "threadId") {
            return Ok(wire::AutomationTargetInput::ThreadHeartbeat { thread_id });
        }
        match args.get("target") {
            Some(Value::String(value)) if value == "project" => {
                Ok(wire::AutomationTargetInput::Project)
            }
            Some(Value::String(value)) if value == "currentThread" => {
                let thread_id = self
                    .current_thread_id
                    .clone()
                    .ok_or_else(|| Error::Message("current thread is not available".to_string()))?;
                Ok(wire::AutomationTargetInput::ThreadHeartbeat { thread_id })
            }
            Some(value) => serde_json::from_value(value.clone()).map_err(Into::into),
            None => Ok(wire::AutomationTargetInput::Project),
        }
    }

    fn scope(&self) -> wire::GatewayRequestScope {
        wire::GatewayRequestScope {
            workdir: self.workdir.display().to_string(),
            source: wire::GatewaySourceInput {
                kind: "web".to_string(),
                raw_id: None,
                lifetime: Some(wire::GatewaySourceLifetime::Persistent),
                raw_identity: None,
                visible_name: None,
            },
        }
    }
}

#[cfg(test)]
pub(super) fn automation_tool_execute_for_test(
    state: WebState,
    workdir: PathBuf,
    current_thread_id: Option<String>,
    args: Value,
) -> psychevo_runtime::Result<Value> {
    AutomationTool {
        state,
        workdir,
        current_thread_id,
    }
    .execute_automation_action(args)
}

fn tool_string(args: &Value, key: &str) -> psychevo_runtime::Result<String> {
    optional_tool_string(args, key)
        .ok_or_else(|| Error::Message(format!("automation tool requires `{key}`")))
}

fn optional_tool_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn tool_result(action: impl Into<String>, value: Value) -> Value {
    let mut value = value;
    let action = action.into();
    if let Value::Object(map) = &mut value {
        map.insert("success".to_string(), Value::Bool(true));
        map.insert("action".to_string(), Value::String(action));
        return value;
    }
    json!({ "success": true, "action": action, "result": value })
}

pub(super) fn automation_list_result(
    state: &WebState,
    auth: &AuthContext,
    params: wire::AutomationListParams,
) -> psychevo_runtime::Result<Value> {
    let scope = resolve_optional_scope(state, auth, params.scope)?;
    let workdir = scope.workdir.display().to_string();
    let automations = state
        .inner
        .state
        .store()
        .automation_tasks_for_workdir(&workdir)?
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
    let (workdir, current_thread_id) = match current_thread_id {
        Some(thread_id) => {
            authorize_thread(&state, auth, &thread_id)?;
            let thread_scope = resolved_scope_for_thread(&state, &thread_id)?;
            authorize_workdir(&state, auth, &thread_scope.workdir)?;
            if let Some(scope) = params.scope {
                let scope = resolve_required_scope(&state, auth, scope)?;
                if scope.workdir != thread_scope.workdir {
                    return Err(Error::Message(
                        "automation draft scope must match current thread workdir".to_string(),
                    ));
                }
            }
            (thread_scope.workdir, Some(thread_id))
        }
        None => {
            let scope = resolve_optional_scope(&state, auth, params.scope)?;
            (scope.workdir, None)
        }
    };

    let mut options = state.run_options(workdir.clone(), None);
    options.prompt = automation_draft_prompt(
        &request,
        &workdir.display().to_string(),
        current_thread_id.as_deref(),
    );
    options.no_agents = true;
    options.no_skills = true;
    options.clarify_enabled = false;
    options.permission_mode = Some(PermissionMode::Default);
    options.sandbox_override = Some(RunSandboxOverride::read_only());
    options.runtime_tools.clear();

    let result = state
        .inner
        .gateway
        .backend
        .run_turn(crate::BackendTurnRequest {
            options,
            runtime_source: "automation-draft".to_string(),
            continue_sources: vec!["automation-draft".to_string()],
            stream: None,
            control: None,
        })
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
    if let Some(existing) = existing.as_ref() {
        authorize_workdir(state, auth, Path::new(&existing.workdir))?;
    }
    let target = resolve_automation_target_scope(state, auth, params.scope, &params.target)?;
    let workdir = target.workdir.display().to_string();
    if let Some(existing) = existing.as_ref()
        && existing.workdir != workdir
    {
        return Err(Error::Message(
            "automation workdir cannot change after creation".to_string(),
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
            workdir,
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
            workdir: existing.workdir,
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

pub(super) async fn run_due_automations_once(state: WebState) -> psychevo_runtime::Result<usize> {
    let now = gateway_now_ms();
    let due = state
        .inner
        .state
        .store()
        .due_automation_tasks(now, AUTOMATION_DUE_LIMIT)?;
    let mut accepted = 0;
    for task in due {
        let schedule = automation_schedule_from_value(task.schedule.clone())?;
        if latest_due_at_ms(&schedule, task.created_at_ms, task.last_run_at_ms, now)?.is_none() {
            continue;
        }
        if start_automation_run(state.clone(), task, "scheduler", None)?.is_some() {
            accepted += 1;
        }
    }
    Ok(accepted)
}

fn start_automation_run(
    state: WebState,
    task: AutomationTaskRecord,
    trigger: &str,
    out_tx: Option<mpsc::UnboundedSender<String>>,
) -> psychevo_runtime::Result<Option<AutomationRunRecord>> {
    let Some(run) = state
        .inner
        .state
        .store()
        .claim_automation_run(&task.id, trigger)?
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
    out_tx: Option<mpsc::UnboundedSender<String>>,
) {
    let result = send_automation_turn(&state, &task).await;
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
                .store()
                .finish_automation_run(AutomationRunFinishInput {
                    run_id: &run.id,
                    status: "completed",
                    thread_id: Some(&thread_id),
                    source_key,
                    error: None,
                    metadata: Some(metadata),
                    next_run_at_ms: next,
                });
            if let Some(out_tx) = out_tx {
                let _ = out_tx.send(rpc_notification(
                    "turn/result",
                    gateway_turn_result_value(turn_result),
                ));
            }
        }
        Err(err) => {
            let next = next_run_after_now(&task).unwrap_or(None);
            let error = err.to_string();
            let _ = state
                .inner
                .state
                .store()
                .finish_automation_run(AutomationRunFinishInput {
                    run_id: &run.id,
                    status: "failed",
                    thread_id: task.target_thread_id.as_deref(),
                    source_key: task.source_key.as_deref(),
                    error: Some(&error),
                    metadata: Some(json!({"trigger": run.trigger})),
                    next_run_at_ms: next,
                });
            if let Some(out_tx) = out_tx {
                let _ = out_tx.send(rpc_notification(
                    "turn/error",
                    json!({"message": error, "threadId": task.target_thread_id}),
                ));
            }
        }
    }
}

async fn send_automation_turn(
    state: &WebState,
    task: &AutomationTaskRecord,
) -> psychevo_runtime::Result<GatewayTurnResult> {
    let workdir = PathBuf::from(&task.workdir);
    let (thread_id, source, bind_source) = match automation_kind_from_str(&task.kind)? {
        wire::AutomationTaskKind::Project => {
            let source = automation_source(&task.id, &task.title);
            let thread_id = state.inner.gateway.resolve_source_thread(&source)?;
            (thread_id, Some(source.clone()), Some(source))
        }
        wire::AutomationTaskKind::ThreadHeartbeat => {
            let thread_id = task.target_thread_id.clone().ok_or_else(|| {
                Error::Message("thread heartbeat automation requires a target thread".to_string())
            })?;
            (Some(thread_id), None, None)
        }
    };
    let mut options = state.run_options(workdir, thread_id.clone());
    options.model = task.model.clone();
    options.reasoning_effort = task.reasoning_effort.clone();
    options.runtime_tools.clear();
    match automation_execution_from_value(task.execution.clone())?.policy {
        wire::AutomationExecutionPolicy::AutoSandbox => {
            options.permission_mode = Some(PermissionMode::BypassPermissions);
            options.sandbox_override = Some(RunSandboxOverride::workspace_write());
        }
        wire::AutomationExecutionPolicy::AskFirst => {
            options.permission_mode = Some(PermissionMode::Default);
        }
    }
    state
        .inner
        .gateway
        .send_turn(crate::SendTurnRequest {
            thread_id,
            source,
            bind_source,
            reset_source_binding: false,
            input: vec![GatewayInputPart::Text {
                text: task.prompt.clone(),
            }],
            options,
            runtime_source: Some("automation".to_string()),
            continue_sources: vec![
                "run".to_string(),
                "tui".to_string(),
                "web".to_string(),
                "automation".to_string(),
            ],
            stream: None,
            event_sink: None,
            control_handle: None,
            control: None,
            lineage: Some(json!({"automationId": task.id})),
        })
        .await
}

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
                workdir: scope.workdir,
                kind: wire::AutomationTaskKind::Project,
                target_thread_id: None,
            })
        }
        wire::AutomationTargetInput::ThreadHeartbeat { thread_id } => {
            authorize_thread(state, auth, thread_id)?;
            let thread_scope = resolved_scope_for_thread(state, thread_id)?;
            authorize_workdir(state, auth, &thread_scope.workdir)?;
            if let Some(scope) = scope {
                let scope = resolve_required_scope(state, auth, scope)?;
                if scope.workdir != thread_scope.workdir {
                    return Err(Error::Message(
                        "automation scope must match target thread workdir".to_string(),
                    ));
                }
            }
            Ok(ResolvedAutomationTarget {
                workdir: thread_scope.workdir,
                kind: wire::AutomationTaskKind::ThreadHeartbeat,
                target_thread_id: Some(thread_id.clone()),
            })
        }
    }
}

struct ResolvedAutomationTarget {
    workdir: PathBuf,
    kind: wire::AutomationTaskKind,
    target_thread_id: Option<String>,
}

fn automation_task_for_request(
    state: &WebState,
    auth: &AuthContext,
    automation_id: &str,
) -> psychevo_runtime::Result<AutomationTaskRecord> {
    let record = state
        .inner
        .state
        .store()
        .automation_task(automation_id)?
        .ok_or_else(|| Error::Message(format!("automation not found: {automation_id}")))?;
    authorize_workdir(state, auth, Path::new(&record.workdir))?;
    Ok(record)
}

fn automation_task_view(
    state: &WebState,
    record: AutomationTaskRecord,
) -> psychevo_runtime::Result<wire::AutomationTaskView> {
    let runs = state
        .inner
        .state
        .store()
        .automation_runs_for_task(&record.id, AUTOMATION_RUN_HISTORY_LIMIT)?
        .into_iter()
        .map(automation_run_view)
        .collect();
    Ok(wire::AutomationTaskView {
        id: record.id,
        workdir: record.workdir,
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

fn automation_draft_prompt(
    request: &str,
    workdir: &str,
    current_thread_id: Option<&str>,
) -> String {
    let thread_guidance = match current_thread_id {
        Some(thread_id) => format!(
            r#"A current thread is available. Use {{"kind":"threadHeartbeat","threadId":"{thread_id}"}} only when the user asks to continue, check, or heartbeat the current thread."#
        ),
        None => {
            "No current thread is available. The target must be {\"kind\":\"project\"}.".to_string()
        }
    };
    format!(
        r#"You draft Psychevo local automations from natural language.
Return only one JSON object. Do not use markdown. Do not call tools.

Rules:
- The draft is not saved yet, so produce editable fields for a confirmation form.
- Prefer a project automation unless the user clearly asks for the current thread.
- If no schedule is specified, use {{"kind":"interval","everyMinutes":60}}.
- For one-shot delay schedules use {{"kind":"delay","afterMinutes":30}}.
- For one-shot absolute schedules use {{"kind":"once","at":"YYYY-MM-DDTHH:mm"}} or an RFC3339 timestamp.
- For daily schedules use {{"kind":"daily","time":"HH:mm"}}.
- For weekly schedules use {{"kind":"weekly","weekdays":[1],"time":"HH:mm"}}, where Monday is 1 and Sunday is 7.
- Interval everyMinutes must be at least 1.
- Default execution is {{"policy":"autoSandbox"}}. Use {{"policy":"askFirst"}} only when the user asks to approve first or review before actions.
- Keep the title short and concrete.
- The prompt must be the exact instruction the agent should run every time, not an explanation of the schedule.
- model and reasoningEffort should be null unless the user explicitly asks for one.

Context:
- Workdir: {workdir}
- {thread_guidance}

Output JSON shape:
{{
  "target": {{"kind":"project"}},
  "title": "Morning repository check",
  "prompt": "Review the current repository state and summarize risks that need attention.",
  "schedule": {{"kind":"interval","everyMinutes":60}},
  "execution": {{"policy":"autoSandbox"}},
  "model": null,
  "reasoningEffort": null
}}

User request:
{request}
"#
    )
}

fn parse_automation_draft_response(
    text: &str,
    current_thread_id: Option<&str>,
) -> psychevo_runtime::Result<wire::AutomationDraftView> {
    let value = extract_json_object(text)?;
    let mut draft: wire::AutomationDraftView = serde_json::from_value(value)?;
    draft.title = draft.title.trim().to_string();
    if draft.title.is_empty() {
        return Err(Error::Message(
            "automation draft is missing a title".to_string(),
        ));
    }
    draft.prompt = draft.prompt.trim().to_string();
    if draft.prompt.is_empty() {
        return Err(Error::Message(
            "automation draft is missing a prompt".to_string(),
        ));
    }
    match &mut draft.target {
        wire::AutomationTargetInput::Project => {}
        wire::AutomationTargetInput::ThreadHeartbeat { thread_id } => {
            let Some(current_thread_id) = current_thread_id else {
                return Err(Error::Message(
                    "automation draft requested a thread target without a current thread"
                        .to_string(),
                ));
            };
            if thread_id.trim().is_empty() {
                *thread_id = current_thread_id.to_string();
            }
            if thread_id != current_thread_id {
                return Err(Error::Message(
                    "automation draft target thread does not match the current thread".to_string(),
                ));
            }
        }
    }
    let schedule = automation_schedule_from_value(serde_json::to_value(&draft.schedule)?)?;
    next_run_at_ms(&schedule, gateway_now_ms(), None, gateway_now_ms())?;
    draft.model = normalize_optional(draft.model);
    draft.reasoning_effort = normalize_reasoning_effort(draft.reasoning_effort);
    Ok(draft)
}

fn extract_json_object(text: &str) -> psychevo_runtime::Result<Value> {
    let trimmed = text.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Ok(value);
    }
    let unfenced = strip_json_fence(trimmed);
    if let Ok(value) = serde_json::from_str::<Value>(unfenced) {
        return Ok(value);
    }
    let start = unfenced.find('{').ok_or_else(|| {
        Error::Message("automation draft response did not contain JSON".to_string())
    })?;
    let end = unfenced.rfind('}').ok_or_else(|| {
        Error::Message("automation draft response did not contain JSON".to_string())
    })?;
    serde_json::from_str(&unfenced[start..=end]).map_err(|err| {
        Error::Message(format!(
            "automation draft response was not valid JSON: {err}"
        ))
    })
}

fn strip_json_fence(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("```") else {
        return text;
    };
    let rest = rest
        .strip_prefix("json")
        .or_else(|| rest.strip_prefix("JSON"))
        .unwrap_or(rest)
        .trim_start();
    rest.rsplit_once("```")
        .map(|(body, _)| body.trim())
        .unwrap_or(text)
}
