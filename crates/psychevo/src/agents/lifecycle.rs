use super::{
    AbortSignal, AgentEdgeRecord, AgentEdgeStatus, AgentRunPhase, AgentRunState, AgentSupervisor,
    BTreeMap, BTreeSet, ContinuationAdmission, ControlHandle, Error, HashMap, Map, Path, PathBuf,
    Result, SessionSummary, StateRuntime, Value, fs, json, user_text_message,
};
use super::{
    catalog_surface::{
        AgentCatalog, AgentDefinition, AgentDiagnostic, AgentInvocationRole, AgentRunRecord,
        AgentRunStatus, AgentSource, AgentToolContext, MAX_AGENT_SPAWN_DEPTH_CAP,
        SUBAGENT_TASK_LABEL_MAX_CHARS,
    },
    child_runs::{ChildRun, bind_child_model, default_task_name, run_child_agent},
    definition_policy::parse_agent_file,
    mailbox_tools::now_ms,
};

pub(crate) async fn force_stop_agent_id(
    supervisor: &AgentSupervisor,
    id: &str,
    store: Option<&StateRuntime>,
) -> Result<Option<AgentRunRecord>> {
    let live = {
        let mut runs = supervisor.slots();
        match resolve_live_key_and_record_locked(&runs, id)? {
            None => None,
            Some((live_id, previous)) => {
                if agent_status_is_final(previous.status) {
                    return Ok(Some(previous));
                }
                let child_session = {
                    let state = runs.get_mut(&live_id).expect("live record exists");
                    if let Some(control) = &state.control {
                        control.stop();
                        control.abort();
                    }
                    state.record.status = AgentRunStatus::Interrupted;
                    state.record.edge_status = Some(AgentEdgeStatus::Closed);
                    state.record.ended_at_ms = Some(now_ms());
                    state.record.outcome = Some("interrupted".to_string());
                    state.record.child_session_id.clone()
                };
                if let Some(child_session) = child_session.as_deref() {
                    interrupt_live_descendants_locked(&mut runs, child_session);
                }
                Some((live_id, previous, child_session))
            }
        }
    };
    let Some((_live_id, previous, _child_session)) = live else {
        return durable_force_stop_agent_id(id, store).await;
    };
    Ok(Some(previous))
}

async fn durable_force_stop_agent_id(
    id: &str,
    store: Option<&StateRuntime>,
) -> Result<Option<AgentRunRecord>> {
    if let Some(store) = store
        && let Some(edge) = find_agent_edge_for_target(store, id).await?
    {
        let previous = agent_record_from_edge(store, edge.clone()).await;
        store
            .close_agent_edge_subtree(&edge.child_session_id)
            .await?;
        return Ok(Some(previous));
    }
    Ok(None)
}

pub(crate) async fn collect_agent_edge_tree(
    store: &StateRuntime,
    parent_session_id: &str,
) -> Result<Vec<AgentEdgeRecord>> {
    let mut records = Vec::new();
    let mut queue = vec![parent_session_id.to_string()];
    let mut seen = BTreeSet::new();
    while let Some(parent) = queue.pop() {
        for edge in store.list_agent_edges_for_parent(&parent).await? {
            if seen.insert(edge.child_session_id.clone()) {
                queue.push(edge.child_session_id.clone());
            }
            records.push(edge);
        }
    }
    Ok(records)
}

pub(crate) fn close_live_descendants_locked(
    runs: &mut HashMap<String, AgentRunState>,
    root_child_session_id: &str,
) {
    let mut sessions = BTreeSet::from([root_child_session_id.to_string()]);
    let mut changed = true;
    while changed {
        changed = false;
        for state in runs.values() {
            if sessions.contains(&state.record.parent_session_id)
                && let Some(child_session) = &state.record.child_session_id
                && sessions.insert(child_session.clone())
            {
                changed = true;
            }
        }
    }

    for state in runs.values_mut() {
        if state.phase != AgentRunPhase::Active {
            continue;
        }
        let child_in_scope = state
            .record
            .child_session_id
            .as_ref()
            .is_some_and(|child| sessions.contains(child));
        if child_in_scope || sessions.contains(&state.record.parent_session_id) {
            if let Some(control) = &state.control {
                control.stop();
            }
            state.record.status = AgentRunStatus::Shutdown;
            state.record.edge_status = Some(AgentEdgeStatus::Closed);
            state.record.ended_at_ms = Some(now_ms());
            state.record.outcome = Some("shutdown".to_string());
        }
    }
}

pub(crate) fn interrupt_live_descendants_locked(
    runs: &mut HashMap<String, AgentRunState>,
    root_child_session_id: &str,
) {
    let mut sessions = BTreeSet::from([root_child_session_id.to_string()]);
    let mut changed = true;
    while changed {
        changed = false;
        for state in runs.values() {
            if sessions.contains(&state.record.parent_session_id)
                && let Some(child_session) = &state.record.child_session_id
                && sessions.insert(child_session.clone())
            {
                changed = true;
            }
        }
    }

    for state in runs.values_mut() {
        if state.phase != AgentRunPhase::Active {
            continue;
        }
        let child_in_scope = state
            .record
            .child_session_id
            .as_ref()
            .is_some_and(|child| sessions.contains(child));
        if child_in_scope || sessions.contains(&state.record.parent_session_id) {
            if agent_status_is_final(state.record.status) {
                continue;
            }
            if let Some(control) = &state.control {
                control.stop();
                control.abort();
            }
            state.record.status = AgentRunStatus::Interrupted;
            state.record.edge_status = Some(AgentEdgeStatus::Closed);
            state.record.ended_at_ms = Some(now_ms());
            state.record.outcome = Some("interrupted".to_string());
        }
    }
}

pub(crate) async fn send_agent_message(
    supervisor: &AgentSupervisor,
    id: &str,
    message: &str,
    store: Option<&StateRuntime>,
) -> Result<Option<AgentRunRecord>> {
    let pending = {
        let runs = supervisor.slots();
        if let Some((live_id, record)) = resolve_live_key_and_record_locked(&runs, id)? {
            let state = runs.get(&live_id).expect("resolved Agent slot");
            if state.phase == AgentRunPhase::Active && !agent_status_is_final(record.status) {
                if let Some(state) = runs.get(&live_id)
                    && let Some(control) = &state.control
                {
                    control
                        .inject_user_message(user_text_message(message.to_string()))
                        .map_err(|error| {
                            Error::Message(format!(
                                "failed to deliver message to agent `{live_id}`: {error}"
                            ))
                        })?;
                }
                return Ok(Some(record));
            }
            if store.is_none() {
                return Ok(Some(record));
            }
            (state.phase == AgentRunPhase::PendingTerminal).then_some(live_id)
        } else {
            None
        }
    };
    if let (Some(store), Some(pending)) = (store, pending) {
        supervisor.retry_terminal(store, &pending).await?;
    }
    if let Some(store) = store
        && let Some(edge) = find_agent_edge_for_target(store, id).await?
    {
        store
            .set_agent_edge_status(&edge.child_session_id, AgentEdgeStatus::Open)
            .await?;
        let mut record = agent_record_from_edge(store, edge).await;
        record.status = AgentRunStatus::PendingInit;
        record.edge_status = Some(AgentEdgeStatus::Open);
        return Ok(Some(record));
    }
    Ok(None)
}

pub(crate) async fn send_agent_message_with_context(
    context: AgentToolContext,
    target: &str,
    message: &str,
    abort: AbortSignal,
) -> Result<Option<AgentRunRecord>> {
    let target = target.trim();
    if target.is_empty() {
        return Ok(None);
    }
    if message.trim().is_empty() {
        return Err(Error::Message("agent message is empty".to_string()));
    }
    let pending = {
        let runs = context.supervisor.slots();
        if let Some((live_id, record)) = resolve_live_key_and_record_locked(&runs, target)? {
            let state = runs.get(&live_id).expect("resolved Agent slot");
            if state.phase == AgentRunPhase::Active && !agent_status_is_final(record.status) {
                if let Some(control) = &state.control {
                    control
                        .inject_user_message(user_text_message(message.to_string()))
                        .map_err(|error| {
                            Error::Message(format!(
                                "failed to deliver message to agent `{live_id}`: {error}"
                            ))
                        })?;
                }
                return Ok(Some(record));
            }
            (state.phase == AgentRunPhase::PendingTerminal).then_some(live_id)
        } else {
            None
        }
    };
    if let Some(pending) = pending {
        context
            .supervisor
            .retry_terminal(&context.state, &pending)
            .await?;
    }

    let Some(edge) = find_agent_edge_for_target(&context.state, target).await? else {
        return Ok(None);
    };
    let base = agent_record_from_edge(&context.state, edge.clone()).await;
    let agent_name = edge_agent_name(&edge).unwrap_or(base.agent_name.as_str());
    let agent = context
        .catalog
        .agents
        .iter()
        .find(|agent| agent.name == agent_name)
        .cloned()
        .ok_or_else(|| Error::Config(format!("unknown agent: {agent_name}")))?;
    let id = base.id;
    let task_name = base
        .task_name
        .clone()
        .unwrap_or_else(|| default_task_name(&agent.name, &id));
    let model_override = context
        .state
        .session_summary(&edge.child_session_id)
        .await?
        .map(|summary| summary.model);
    let (child_model, child_provider) =
        bind_child_model(&context, &agent, model_override.as_deref())?;
    let spawn_depth_remaining = edge_spawn_depth_remaining(&edge);
    let record = AgentRunRecord {
        id: id.clone(),
        task_name: Some(task_name.clone()),
        agent_name: agent.name.clone(),
        task: message.to_string(),
        parent_session_id: edge.parent_session_id.clone(),
        child_session_id: Some(edge.child_session_id.clone()),
        role: base.role,
        background: true,
        status: AgentRunStatus::Running,
        edge_status: Some(AgentEdgeStatus::Open),
        started_at_ms: now_ms(),
        ended_at_ms: None,
        outcome: None,
        final_answer: None,
        error: None,
        effective_max_spawn_depth: Some(spawn_depth_remaining),
        team_run_id: base.team_run_id.clone(),
        mission_run_id: base.mission_run_id.clone(),
        team_name: base.team_name.clone(),
        team_member_id: base.team_member_id.clone(),
        agent_path: base.agent_path.clone(),
    };
    let (control_handle, control_receivers) = ControlHandle::new();
    let concurrency_limit = context
        .active_team
        .as_ref()
        .map(|team| team.max_parallel_agents as usize)
        .unwrap_or(super::supervisor::DEFAULT_CHILD_CONCURRENCY);
    match context.supervisor.register_continuation_or_inject(
        record.clone(),
        control_handle.clone(),
        user_text_message(message.to_string()),
        concurrency_limit,
    )? {
        ContinuationAdmission::Registered => {}
        ContinuationAdmission::Injected(active) => return Ok(Some(*active)),
    }
    let mut child_context = context;
    child_context.parent_session_id = edge.parent_session_id.clone();
    let child = ChildRun {
        id,
        context: child_context,
        agent,
        prompt: message.to_string(),
        task_name,
        model: child_model,
        provider: child_provider,
        fork_context: false,
        fork_turns: None,
        max_turns: None,
        spawn_depth_remaining,
        role: base.role,
        background: true,
        team_member_id: base.team_member_id.clone(),
        parent_tool_call_id: None,
        existing_child_session: Some(edge.child_session_id),
        previous_messages_override: None,
        control_handle,
        control_receivers,
        abort,
    };
    let supervisor = child.context.supervisor.clone();
    if let Err(finalizer) = supervisor.spawn_background(Box::pin(async move {
        let _ = run_child_agent(child).await;
    })) {
        finalizer.await;
        return Err(Error::Message(
            "agent supervisor is shutting down".to_string(),
        ));
    }
    Ok(Some(record))
}

pub(crate) async fn resume_agent_id(
    supervisor: &AgentSupervisor,
    id: &str,
    store: Option<&StateRuntime>,
) -> Result<Option<AgentRunRecord>> {
    if let Some(record) = {
        let runs = supervisor.slots();
        resolve_live_record_locked(&runs, id)?
    } {
        return Ok(Some(record));
    }
    if let Some(store) = store
        && let Some(edge) = find_agent_edge_for_target(store, id).await?
    {
        store
            .set_agent_edge_status(&edge.child_session_id, AgentEdgeStatus::Open)
            .await?;
        let mut record = agent_record_from_edge(store, edge).await;
        record.edge_status = Some(AgentEdgeStatus::Open);
        return Ok(Some(record));
    }
    Ok(None)
}

pub(crate) fn edge_agent_name(edge: &AgentEdgeRecord) -> Option<&str> {
    edge.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("agent"))
        .and_then(Value::as_object)
        .and_then(|agent| agent.get("name"))
        .and_then(Value::as_str)
}

pub(crate) fn edge_spawn_depth_remaining(edge: &AgentEdgeRecord) -> u8 {
    edge.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("agent"))
        .and_then(Value::as_object)
        .and_then(|agent| {
            agent
                .get("effective_max_spawn_depth")
                .or_else(|| agent.get("max_spawn_depth"))
        })
        .and_then(Value::as_u64)
        .map(|value| (value as u8).min(MAX_AGENT_SPAWN_DEPTH_CAP))
        .unwrap_or(0)
}

pub(crate) fn resolve_live_key_and_record_locked(
    runs: &HashMap<String, AgentRunState>,
    target: &str,
) -> Result<Option<(String, AgentRunRecord)>> {
    let target = target.trim();
    if target.is_empty() {
        return Ok(None);
    }
    if let Some((id, state)) = runs.iter().find(|(id, state)| {
        id.as_str() == target
            || state.record.id == target
            || state.record.child_session_id.as_deref() == Some(target)
            || generated_task_name_matches(&state.record, target)
    }) {
        return Ok(Some((id.clone(), state.record.clone())));
    }

    let matches = runs
        .iter()
        .filter(|(_, state)| record_task_label(&state.record) == target)
        .map(|(id, state)| (id.clone(), state.record.clone()))
        .collect::<Vec<_>>();
    match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.into_iter().next()),
        _ => Err(ambiguous_agent_task_error(target)),
    }
}

pub(crate) fn resolve_live_record_locked(
    runs: &HashMap<String, AgentRunState>,
    target: &str,
) -> Result<Option<AgentRunRecord>> {
    resolve_live_key_and_record_locked(runs, target).map(|record| record.map(|(_, record)| record))
}

pub(crate) fn generated_task_name_matches(record: &AgentRunRecord, target: &str) -> bool {
    record.task_name.as_deref() == Some(target) && explicit_record_task_name(record).is_none()
}

pub(crate) async fn find_agent_edge_for_target(
    store: &StateRuntime,
    target: &str,
) -> Result<Option<AgentEdgeRecord>> {
    let target = target.trim();
    if target.is_empty() {
        return Ok(None);
    }
    let edges = store.list_agent_edges().await?;
    if let Some(edge) = edges
        .iter()
        .find(|edge| agent_edge_exact_target_matches(edge, target))
    {
        return Ok(Some(edge.clone()));
    }

    let mut matches = Vec::new();
    for edge in edges {
        if record_task_label(&agent_record_from_edge(store, edge.clone()).await) == target {
            matches.push(edge);
        }
    }
    match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.into_iter().next()),
        _ => Err(ambiguous_agent_task_error(target)),
    }
}

pub(crate) fn agent_edge_exact_target_matches(edge: &AgentEdgeRecord, target: &str) -> bool {
    edge.child_session_id == target
        || edge_agent_id(edge).is_some_and(|value| value == target)
        || edge_generated_task_name_matches(edge, target)
}

pub(crate) fn edge_generated_task_name_matches(edge: &AgentEdgeRecord, target: &str) -> bool {
    let Some(task_name) = edge_task_name(edge) else {
        return false;
    };
    if task_name != target {
        return false;
    }
    let id = edge_agent_id(edge).unwrap_or(edge.child_session_id.as_str());
    let agent_name = edge_agent_name(edge).unwrap_or("agent");
    task_name == default_task_name(agent_name, id)
}

pub(crate) fn edge_agent_id(edge: &AgentEdgeRecord) -> Option<&str> {
    edge.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("agent"))
        .and_then(Value::as_object)
        .and_then(|agent| agent.get("id"))
        .and_then(Value::as_str)
}

pub(crate) fn edge_task_name(edge: &AgentEdgeRecord) -> Option<&str> {
    edge.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("agent"))
        .and_then(Value::as_object)
        .and_then(|agent| agent.get("task_name"))
        .and_then(Value::as_str)
}

pub(crate) fn edge_team_run_id(edge: &AgentEdgeRecord) -> Option<&str> {
    edge.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("teamRunId"))
        .and_then(Value::as_str)
}

pub(crate) fn edge_mission_run_id(edge: &AgentEdgeRecord) -> Option<&str> {
    edge.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("missionRunId"))
        .and_then(Value::as_str)
}

pub(crate) fn edge_team_name(edge: &AgentEdgeRecord) -> Option<&str> {
    edge.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("teamName"))
        .and_then(Value::as_str)
}

pub(crate) fn edge_team_member_id(edge: &AgentEdgeRecord) -> Option<&str> {
    edge.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("teamMemberId"))
        .and_then(Value::as_str)
}

pub(crate) fn ambiguous_agent_task_error(target: &str) -> Error {
    Error::Config(format!(
        "multiple agents match task `{target}`; use agent_id"
    ))
}

pub(crate) async fn agent_record_from_edge(
    store: &StateRuntime,
    edge: AgentEdgeRecord,
) -> AgentRunRecord {
    let agent = edge
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("agent"))
        .and_then(Value::as_object);
    let summary = store
        .session_summary(&edge.child_session_id)
        .await
        .ok()
        .flatten();
    let id = agent
        .and_then(|agent| agent.get("id"))
        .and_then(Value::as_str)
        .unwrap_or(edge.child_session_id.as_str())
        .to_string();
    let status = if edge.status == AgentEdgeStatus::Closed {
        AgentRunStatus::Shutdown
    } else {
        match summary
            .as_ref()
            .and_then(|summary| summary.end_reason.as_deref())
        {
            Some("normal") => AgentRunStatus::Completed,
            Some("stopped") | Some("aborted") => AgentRunStatus::Interrupted,
            Some(_) => AgentRunStatus::Errored,
            None => AgentRunStatus::Interrupted,
        }
    };
    let effective_max_spawn_depth = edge_spawn_depth_remaining(&edge);
    AgentRunRecord {
        id,
        task_name: agent
            .and_then(|agent| agent.get("task_name"))
            .and_then(Value::as_str)
            .map(str::to_string),
        agent_name: agent
            .and_then(|agent| agent.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("agent")
            .to_string(),
        task: agent
            .and_then(|agent| agent.get("task"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        parent_session_id: edge.parent_session_id.clone(),
        child_session_id: Some(edge.child_session_id.clone()),
        role: agent
            .and_then(|agent| agent.get("role"))
            .and_then(Value::as_str)
            .and_then(parse_invocation_role)
            .unwrap_or(AgentInvocationRole::Subagent),
        background: agent
            .and_then(|agent| agent.get("background"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        status,
        edge_status: Some(edge.status),
        started_at_ms: summary
            .as_ref()
            .map(|summary| summary.started_at_ms)
            .unwrap_or(edge.created_at_ms),
        ended_at_ms: summary.as_ref().and_then(|summary| summary.ended_at_ms),
        outcome: summary.and_then(|summary| summary.end_reason),
        final_answer: None,
        error: None,
        effective_max_spawn_depth: Some(effective_max_spawn_depth),
        team_run_id: edge_team_run_id(&edge).map(str::to_string),
        mission_run_id: edge_mission_run_id(&edge).map(str::to_string),
        team_name: edge_team_name(&edge).map(str::to_string),
        team_member_id: edge_team_member_id(&edge).map(str::to_string),
        agent_path: agent
            .and_then(|agent| agent.get("agent_path"))
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

pub(crate) async fn agent_child_session_summary_value(
    store: &StateRuntime,
    summary: &SessionSummary,
) -> Value {
    let latest_usage = latest_session_assistant_usage(store, &summary.id).await;
    let latest_total_tokens = latest_usage.as_ref().and_then(usage_total_tokens);
    let mut value = json!({
        "id": summary.id,
        "message_count": summary.message_count,
        "tool_call_count": summary.tool_call_count,
    });
    if let Some(object) = value.as_object_mut() {
        if let Some(usage) = latest_usage {
            object.insert("latest_usage".to_string(), usage);
        }
        if let Some(tokens) = latest_total_tokens {
            object.insert("latest_total_tokens".to_string(), Value::from(tokens));
        }
    }
    value
}

pub(crate) async fn subagent_summary_value(
    store: Option<&StateRuntime>,
    record: &AgentRunRecord,
    include_agent_id: bool,
) -> Value {
    let mut object = Map::new();
    if include_agent_id {
        object.insert("agent_id".to_string(), Value::from(record.id.clone()));
    }
    object.insert(
        "agent_name".to_string(),
        Value::from(record.agent_name.clone()),
    );
    object.insert(
        "task_name".to_string(),
        Value::from(
            record
                .task_name
                .clone()
                .unwrap_or_else(|| record_task_label(record)),
        ),
    );
    object.insert("status".to_string(), Value::from(record.status.as_str()));
    if let Some(exit_reason) = record_exit_reason(record) {
        object.insert("exit_reason".to_string(), Value::from(exit_reason));
    }
    if let Some(summary) = &record.final_answer {
        object.insert("summary".to_string(), Value::from(summary.clone()));
    }
    if let Some(duration_ms) = record_duration_ms(record) {
        object.insert("duration_ms".to_string(), Value::from(duration_ms));
    }
    if let Some(store) = store
        && let Some(child_session_id) = record.child_session_id.as_deref()
    {
        if let Ok(Some(summary)) = store.session_summary(child_session_id).await {
            object.insert(
                "tool_call_count".to_string(),
                Value::from(summary.tool_call_count),
            );
            object.insert("model".to_string(), Value::from(summary.model));
        }
        if let Some(tokens) = child_session_tokens_value(store, child_session_id).await {
            object.insert("tokens".to_string(), tokens);
        }
    }
    if let Some(error) = &record.error
        && !error.is_empty()
    {
        object.insert("error".to_string(), Value::from(error.clone()));
    }
    Value::Object(object)
}

pub(crate) fn model_content_string(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{\"error\":\"invalid result\"}".to_string())
}

pub(crate) fn record_task_label(record: &AgentRunRecord) -> String {
    if let Some(task_name) = explicit_record_task_name(record) {
        return collapse_and_cap_task_label(task_name);
    }
    first_prompt_task_line(&record.task)
        .map(collapse_and_cap_task_label)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| collapse_and_cap_task_label(&record.agent_name))
}

pub(crate) fn explicit_record_task_name(record: &AgentRunRecord) -> Option<&str> {
    let task_name = record
        .task_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let generated = default_task_name(&record.agent_name, &record.id);
    (task_name != generated).then_some(task_name)
}

pub(crate) fn first_prompt_task_line(prompt: &str) -> Option<&str> {
    prompt.lines().map(str::trim).find(|line| !line.is_empty())
}

pub(crate) fn collapse_and_cap_task_label(value: &str) -> String {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= SUBAGENT_TASK_LABEL_MAX_CHARS {
        return collapsed;
    }
    collapsed
        .chars()
        .take(SUBAGENT_TASK_LABEL_MAX_CHARS)
        .collect()
}

pub(crate) fn record_exit_reason(record: &AgentRunRecord) -> Option<String> {
    if let Some(outcome) = record.outcome.as_deref().map(str::trim)
        && !outcome.is_empty()
    {
        return Some(outcome.to_string());
    }
    match record.status {
        AgentRunStatus::Completed => Some("completed".to_string()),
        AgentRunStatus::Errored => Some("failed".to_string()),
        AgentRunStatus::Interrupted => Some("interrupted".to_string()),
        AgentRunStatus::Shutdown => Some("shutdown".to_string()),
        AgentRunStatus::NotFound => Some("not_found".to_string()),
        AgentRunStatus::PendingInit | AgentRunStatus::Running => None,
    }
}

pub(crate) fn record_duration_ms(record: &AgentRunRecord) -> Option<u64> {
    let ended_at_ms = record.ended_at_ms?;
    Some(ended_at_ms.saturating_sub(record.started_at_ms).max(0) as u64)
}

pub(crate) async fn child_session_tokens_value(
    store: &StateRuntime,
    session_id: &str,
) -> Option<Value> {
    crate::stats::session_agent_token_totals(store, session_id)
        .await
        .ok()
        .flatten()
}

pub(crate) async fn latest_session_assistant_usage(
    store: &StateRuntime,
    session_id: &str,
) -> Option<Value> {
    store.latest_assistant_usage(session_id).await.ok()?
}

pub(crate) fn usage_total_tokens(usage: &Value) -> Option<u64> {
    crate::accounting::effective_usage_total(Some(usage)).tokens
}

pub(crate) fn parse_invocation_role(value: &str) -> Option<AgentInvocationRole> {
    match value {
        "main" => Some(AgentInvocationRole::Main),
        "child" | "subagent" => Some(AgentInvocationRole::Subagent),
        "fork" => Some(AgentInvocationRole::Fork),
        "system" => Some(AgentInvocationRole::System),
        _ => None,
    }
}

pub(crate) fn agent_status_is_final(status: AgentRunStatus) -> bool {
    matches!(
        status,
        AgentRunStatus::Completed
            | AgentRunStatus::Errored
            | AgentRunStatus::Interrupted
            | AgentRunStatus::Shutdown
            | AgentRunStatus::NotFound
    )
}

pub(crate) fn load_agent_dir(
    catalog: &mut AgentCatalog,
    winners: &mut BTreeMap<String, PathBuf>,
    dir: &Path,
    source: AgentSource,
) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let mut paths = Vec::new();
    collect_agent_markdown_files(dir, &mut paths)?;
    paths.sort();
    for path in paths {
        load_agent_file(catalog, winners, &path, source)?;
    }
    Ok(())
}

pub(crate) fn collect_agent_markdown_files(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_agent_markdown_files(&path, paths)?;
        } else if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            paths.push(path);
        }
    }
    Ok(())
}

pub(crate) fn load_agent_file(
    catalog: &mut AgentCatalog,
    winners: &mut BTreeMap<String, PathBuf>,
    path: &Path,
    source: AgentSource,
) -> Result<()> {
    match parse_agent_file(path, source) {
        Ok(agent) => insert_agent(catalog, winners, agent),
        Err(err) => catalog.diagnostics.push(AgentDiagnostic::warning(
            format!("failed to load agent {}: {err}", path.display()),
            Some(path.to_path_buf()),
        )),
    }
    Ok(())
}

pub(crate) fn insert_agent(
    catalog: &mut AgentCatalog,
    winners: &mut BTreeMap<String, PathBuf>,
    agent: AgentDefinition,
) {
    if !agent.enabled {
        catalog.disabled_agents.push(agent);
        return;
    }
    let loser_path = agent
        .file_path
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("<{}>", agent.source.as_str())));
    if let Some(winner) = winners.get(&agent.name) {
        catalog
            .diagnostics
            .push(AgentDiagnostic::collision(&agent.name, winner, &loser_path));
        catalog.shadowed_agents.push(agent);
        return;
    }
    winners.insert(agent.name.clone(), loser_path);
    catalog.agents.push(agent);
}
