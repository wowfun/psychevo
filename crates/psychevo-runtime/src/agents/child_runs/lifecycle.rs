#[allow(unused_imports)]
pub(crate) use super::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SpawnAgentArgs {
    #[serde(default)]
    pub(crate) agent_type: Option<String>,
    pub(crate) task_name: String,
    pub(crate) message: String,
    #[serde(default)]
    pub(crate) background: Option<bool>,
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) fork_context: bool,
    #[serde(default)]
    pub(crate) fork_turns: Option<String>,
    #[serde(default)]
    pub(crate) max_turns: Option<usize>,
    #[serde(default)]
    pub(crate) max_spawn_depth: Option<u8>,
}

pub(crate) async fn spawn_subagent(
    context: AgentToolContext,
    args: SpawnAgentArgs,
    tool_call_id: String,
    abort: AbortSignal,
) -> Result<ToolOutput> {
    if args.message.trim().is_empty() {
        return Err(Error::Message("spawn_agent message is empty".to_string()));
    }
    validate_task_name(&args.task_name)?;
    if agent_spawn_paused() {
        return Err(Error::Config("agent spawning is paused".to_string()));
    }
    if context.spawn_depth_remaining == Some(0) {
        return Err(Error::Config(
            "agent spawning is disabled for this child agent".to_string(),
        ));
    }
    let agent_name = resolve_agent_tool_name(&args, &context.required_agent_names)?;
    let agent = context
        .catalog
        .agents
        .iter()
        .find(|agent| agent.name == agent_name)
        .cloned()
        .ok_or_else(|| Error::Config(format!("unknown agent: {agent_name}")))?;
    if !agent.supports_entrypoint(AgentEntrypoint::Subagent) {
        return Err(Error::Config(format!(
            "agent `{}` does not support subagent execution",
            agent.name
        )));
    }
    if let Some(allowed) = &context.allowed_agent_names
        && !allowed.contains(&agent.name)
    {
        return Err(Error::Config(format!(
            "agent `{}` is not allowed by selected-agent tool policy",
            agent.name
        )));
    }
    if context.denied_agent_names.contains(&agent.name) {
        return Err(Error::Config(format!(
            "agent `{}` is denied by selected-agent tool policy",
            agent.name
        )));
    }
    if agent.backend.is_some() {
        return spawn_external_subagent(context, args, tool_call_id, abort, agent).await;
    }
    let id = Uuid::now_v7().to_string();
    let task_name = args.task_name.trim().to_string();
    let spawn_depth_remaining = child_spawn_depth_remaining(&context, &agent, args.max_spawn_depth);
    let background =
        args.fork_context || agent.background.unwrap_or(false) || args.background.unwrap_or(false);
    let role = if args.fork_context {
        AgentInvocationRole::Fork
    } else {
        AgentInvocationRole::Subagent
    };
    let precreated_child_session = if background {
        Some(create_internal_child_session(InternalChildSessionInput {
            context: &context,
            agent: &agent,
            id: &id,
            task_name: &task_name,
            prompt: &args.message,
            model_override: args.model.as_deref(),
            role,
            background,
            fork_context: args.fork_context,
            spawn_depth_remaining,
            parent_tool_call_id: Some(&tool_call_id),
        })?)
    } else {
        None
    };
    let previous_messages_override = precreated_child_session.as_ref().map(|_| {
        fork_messages(
            &context.parent_context_snapshot,
            args.fork_context,
            args.fork_turns.as_deref(),
        )
    });
    let record = AgentRunRecord {
        id: id.clone(),
        task_name: Some(task_name.clone()),
        agent_name: agent.name.clone(),
        task: args.message.clone(),
        parent_session_id: context.parent_session_id.clone(),
        child_session_id: precreated_child_session.clone(),
        role,
        background,
        status: AgentRunStatus::Running,
        edge_status: Some(AgentEdgeStatus::Open),
        started_at_ms: now_ms(),
        ended_at_ms: None,
        outcome: None,
        final_answer: None,
        error: None,
        effective_max_spawn_depth: Some(spawn_depth_remaining),
    };
    let response_record = record.clone();
    let (control_handle, control_receivers) = ControlHandle::new();
    {
        let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
        runs.insert(
            id.clone(),
            AgentRunState {
                record,
                control: Some(control_handle.clone()),
            },
        );
    }

    let response_agent_name = agent.name.clone();
    let response_agent_description = agent.description.clone();
    let response_task_name = task_name.clone();
    let response_store = context.state.store().clone();
    let response_child_session_id = precreated_child_session.clone();
    let response_tool_call_id = tool_call_id.clone();
    let parent_abort_bridge = if background {
        None
    } else {
        Some(spawn_parent_abort_bridge(
            abort.clone(),
            control_handle.clone(),
        ))
    };
    let child = ChildRun {
        id: id.clone(),
        context,
        agent,
        prompt: args.message,
        task_name,
        model_override: args.model,
        fork_context: args.fork_context,
        fork_turns: args.fork_turns,
        max_turns: args.max_turns,
        spawn_depth_remaining,
        role,
        background,
        parent_tool_call_id: Some(tool_call_id),
        existing_child_session: precreated_child_session,
        previous_messages_override,
        control_receivers,
        abort,
    };

    if background {
        tokio::spawn(async move {
            let _ = run_child_agent(child).await;
        });
        let system_value = json!({
            "id": id,
            "agent_name": response_agent_name.clone(),
            "agent_description": response_agent_description,
            "agent_type": response_agent_name,
            "agent_path": agent_path(&response_task_name),
            "task_name": response_task_name,
            "message": response_record.task.clone(),
            "task": response_record.task.clone(),
            "tool_call_id": response_tool_call_id,
            "parent_thread_id": response_record.parent_session_id.clone(),
            "status": "running",
            "background": true,
            "session_id": response_child_session_id,
            "child_thread_id": response_record.child_session_id.clone(),
            "child_session_id": response_record.child_session_id.clone(),
            "effective_max_spawn_depth": spawn_depth_remaining
        });
        let model_value = subagent_summary_value(Some(&response_store), &response_record, true);
        Ok(ToolOutput::ok_with_model_content(
            system_value,
            model_content_string(&model_value),
        ))
    } else {
        let record = run_child_agent(child).await;
        if let Some(handle) = parent_abort_bridge {
            handle.abort();
        }
        let record = record?;
        let model_value = subagent_summary_value(Some(&response_store), &record, false);
        let response_child_session_id = record.child_session_id.clone();
        let child_summary = record
            .child_session_id
            .as_deref()
            .and_then(|session_id| response_store.session_summary(session_id).ok().flatten())
            .map(|summary| agent_child_session_summary_value(&response_store, &summary));
        let system_value = json!({
            "id": record.id,
            "agent_name": record.agent_name.clone(),
            "agent_description": response_agent_description,
            "agent_type": record.agent_name,
            "agent_path": record.task_name.as_deref().map(agent_path),
            "task_name": record.task_name,
            "message": record.task.clone(),
            "task": record.task,
            "parent_thread_id": record.parent_session_id,
            "status": record.status.as_str(),
            "background": false,
            "session_id": response_child_session_id,
            "child_thread_id": record.child_session_id.clone(),
            "child_session_id": record.child_session_id,
            "outcome": record.outcome,
            "final_answer": record.final_answer,
            "error": record.error,
            "child_session": child_summary,
            "effective_max_spawn_depth": record.effective_max_spawn_depth,
        });
        Ok(ToolOutput::ok_with_model_content(
            system_value,
            model_content_string(&model_value),
        ))
    }
}

struct InternalChildSessionInput<'a> {
    context: &'a AgentToolContext,
    agent: &'a AgentDefinition,
    id: &'a str,
    task_name: &'a str,
    prompt: &'a str,
    model_override: Option<&'a str>,
    role: AgentInvocationRole,
    background: bool,
    fork_context: bool,
    spawn_depth_remaining: u8,
    parent_tool_call_id: Option<&'a str>,
}

fn create_internal_child_session(input: InternalChildSessionInput<'_>) -> Result<String> {
    let context = input.context;
    let agent = input.agent;
    let child_model = child_model_from(context, agent, input.model_override);
    let mut metadata = child_agent_metadata(ChildAgentMetadataInput {
        id: input.id,
        task_name: input.task_name,
        agent,
        parent_session_id: &context.parent_session_id,
        role: input.role,
        task: input.prompt,
        background: input.background,
        fork_context: input.fork_context,
        spawn_depth_remaining: input.spawn_depth_remaining,
        context: Some(context),
        parent_tool_call_id: input.parent_tool_call_id,
    });
    let child_session = context.state.store().create_child_session_with_metadata(
        &context.parent_session_id,
        &context.cwd,
        "agent",
        &child_model,
        &context.model_provider,
        Some(metadata.clone()),
    )?;
    attach_child_thread_metadata(&mut metadata, &child_session);
    context.state.store().upsert_agent_edge(
        &context.parent_session_id,
        &child_session,
        AgentEdgeStatus::Open,
        Some(metadata),
    )?;
    Ok(child_session)
}

fn attach_child_thread_metadata(metadata: &mut Value, child_session: &str) {
    if let Some(object) = metadata.as_object_mut() {
        object.insert(
            "child_thread_id".to_string(),
            Value::String(child_session.to_string()),
        );
        object.insert(
            "child_session_id".to_string(),
            Value::String(child_session.to_string()),
        );
        if let Some(agent) = object.get_mut("agent").and_then(Value::as_object_mut) {
            agent.insert(
                "child_thread_id".to_string(),
                Value::String(child_session.to_string()),
            );
            agent.insert(
                "child_session_id".to_string(),
                Value::String(child_session.to_string()),
            );
        }
    }
}

fn spawn_parent_abort_bridge(
    mut parent_abort: AbortSignal,
    child_control: ControlHandle,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        parent_abort.wait_for_abort().await;
        child_control.abort();
    })
}

async fn spawn_external_subagent(
    context: AgentToolContext,
    args: SpawnAgentArgs,
    tool_call_id: String,
    abort: AbortSignal,
    agent: AgentDefinition,
) -> Result<ToolOutput> {
    if abort.aborted() {
        return Err(Error::Message("parent invocation aborted".to_string()));
    }
    if args.fork_context {
        return Ok(ToolOutput::error(format!(
            "agent `{}` is backed by an external ACP backend and does not support fork_context; run it as a foreground delegated task",
            agent.name
        )));
    }
    if args.background.unwrap_or(false) || agent.background.unwrap_or(false) {
        return Ok(ToolOutput::error(format!(
            "agent `{}` is backed by an external ACP backend and does not support background delegation yet; run it as a foreground delegated task",
            agent.name
        )));
    }
    let Some(delegate) = context.external_delegate.clone() else {
        return Ok(ToolOutput::error(format!(
            "agent `{}` is backed by an external ACP backend, but this execution context cannot delegate to peer agents",
            agent.name
        )));
    };
    let Some(backend) = agent.backend.as_ref() else {
        return Ok(ToolOutput::error(format!(
            "agent `{}` is missing backend.ref",
            agent.name
        )));
    };

    let id = Uuid::now_v7().to_string();
    let task_name = args.task_name.trim().to_string();
    let spawn_depth_remaining = child_spawn_depth_remaining(&context, &agent, args.max_spawn_depth);
    let mut metadata = child_agent_metadata(ChildAgentMetadataInput {
        id: &id,
        task_name: &task_name,
        agent: &agent,
        parent_session_id: &context.parent_session_id,
        role: AgentInvocationRole::Subagent,
        task: &args.message,
        background: false,
        fork_context: false,
        spawn_depth_remaining,
        context: Some(&context),
        parent_tool_call_id: Some(&tool_call_id),
    });
    let child_session = context.state.store().create_child_session_with_metadata(
        &context.parent_session_id,
        &context.cwd,
        "peer_agent",
        &agent.name,
        &format!("acp:{}", backend.name),
        Some(metadata.clone()),
    )?;
    attach_child_thread_metadata(&mut metadata, &child_session);
    context.state.store().upsert_agent_edge(
        &context.parent_session_id,
        &child_session,
        AgentEdgeStatus::Open,
        Some(metadata),
    )?;

    let record = AgentRunRecord {
        id: id.clone(),
        task_name: Some(task_name.clone()),
        agent_name: agent.name.clone(),
        task: args.message.clone(),
        parent_session_id: context.parent_session_id.clone(),
        child_session_id: Some(child_session.clone()),
        role: AgentInvocationRole::Subagent,
        background: false,
        status: AgentRunStatus::Running,
        edge_status: Some(AgentEdgeStatus::Open),
        started_at_ms: now_ms(),
        ended_at_ms: None,
        outcome: None,
        final_answer: None,
        error: None,
        effective_max_spawn_depth: Some(spawn_depth_remaining),
    };
    {
        let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
        runs.insert(
            id.clone(),
            AgentRunState {
                record,
                control: None,
            },
        );
    }
    emit_external_agent_session_start(ExternalAgentSessionStart {
        context: &context,
        agent: &agent,
        id: &id,
        task_name: &task_name,
        task: &args.message,
        tool_call_id: &tool_call_id,
        child_session_id: &child_session,
        spawn_depth_remaining,
    });

    let request = ExternalAgentDelegateRequest {
        run_id: id.clone(),
        parent_session_id: context.parent_session_id.clone(),
        child_session_id: child_session.clone(),
        agent_name: agent.name.clone(),
        agent_description: agent.description.clone(),
        backend_ref: backend.name.clone(),
        prompt: args.message.clone(),
        task_name,
        model: args.model.clone(),
        runtime_options: BTreeMap::new(),
        abort,
    };
    let result = delegate.run(request).await;
    let record = match result {
        Ok(result) => {
            let record = update_run_completed(&id, result.outcome, result.final_answer.clone());
            let _ = context
                .state
                .store()
                .set_agent_edge_status(&result.child_session_id, AgentEdgeStatus::Closed);
            record
        }
        Err(err) => {
            update_run_failed(&id, &err.to_string());
            let _ = context
                .state
                .store()
                .set_agent_edge_status(&child_session, AgentEdgeStatus::Closed);
            let record = {
                let runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
                runs.get(&id)
                    .map(|state| state.record.clone())
                    .unwrap_or_else(|| AgentRunRecord {
                        id: id.clone(),
                        task_name: None,
                        agent_name: agent.name.clone(),
                        task: args.message.clone(),
                        parent_session_id: context.parent_session_id.clone(),
                        child_session_id: Some(child_session.clone()),
                        role: AgentInvocationRole::Subagent,
                        background: false,
                        status: AgentRunStatus::Errored,
                        edge_status: Some(AgentEdgeStatus::Closed),
                        started_at_ms: now_ms(),
                        ended_at_ms: Some(now_ms()),
                        outcome: Some("failed".to_string()),
                        final_answer: None,
                        error: Some(err.to_string()),
                        effective_max_spawn_depth: Some(spawn_depth_remaining),
                    })
            };
            let model_value = subagent_summary_value(Some(context.state.store()), &record, false);
            return Ok(ToolOutput::error(model_content_string(&model_value)));
        }
    };
    let model_value = subagent_summary_value(Some(context.state.store()), &record, false);
    let child_summary = record
        .child_session_id
        .as_deref()
        .and_then(|session_id| {
            context
                .state
                .store()
                .session_summary(session_id)
                .ok()
                .flatten()
        })
        .map(|summary| agent_child_session_summary_value(context.state.store(), &summary));
    let response_child_session_id = record.child_session_id.clone();
    let system_value = json!({
        "id": record.id,
        "agent_name": record.agent_name.clone(),
        "agent_description": agent.description,
        "agent_type": record.agent_name,
        "agent_path": record.task_name.as_deref().map(agent_path),
        "task_name": record.task_name,
        "message": record.task.clone(),
        "task": record.task,
        "parent_thread_id": record.parent_session_id,
        "status": record.status.as_str(),
        "background": false,
        "session_id": response_child_session_id,
        "child_thread_id": record.child_session_id.clone(),
        "child_session_id": record.child_session_id,
        "outcome": record.outcome,
        "final_answer": record.final_answer,
        "error": record.error,
        "child_session": child_summary,
        "effective_max_spawn_depth": record.effective_max_spawn_depth,
    });
    Ok(ToolOutput::ok_with_model_content(
        system_value,
        model_content_string(&model_value),
    ))
}

pub(crate) fn resolve_agent_tool_name(
    args: &SpawnAgentArgs,
    required_agent_names: &[String],
) -> Result<String> {
    let agent_type = normalized_optional_name(args.agent_type.as_deref());
    if let Some(name) = agent_type {
        return Ok(name);
    }
    match required_agent_names {
        [single] => Ok(single.clone()),
        [] => Ok("general".to_string()),
        many => Err(Error::Config(format!(
            "spawn_agent call must set agent_type when the user mentioned multiple agents: {}",
            many.join(", ")
        ))),
    }
}

pub(crate) fn normalized_optional_name(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn validate_task_name(value: &str) -> Result<()> {
    let value = value.trim();
    if value.is_empty() || matches!(value, "root" | "." | "..") {
        return Err(Error::Message(
            "task_name must use lowercase letters, digits, and underscores".to_string(),
        ));
    }
    if value
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
    {
        Ok(())
    } else {
        Err(Error::Message(
            "task_name must use lowercase letters, digits, and underscores".to_string(),
        ))
    }
}

pub(crate) fn child_spawn_depth_remaining(
    context: &AgentToolContext,
    agent: &AgentDefinition,
    requested: Option<u8>,
) -> u8 {
    resolved_child_spawn_depth_remaining(
        context.spawn_depth_remaining,
        agent.max_spawn_depth,
        requested,
    )
}

pub(crate) fn resolved_child_spawn_depth_remaining(
    parent_remaining: Option<u8>,
    definition_depth: u8,
    requested: Option<u8>,
) -> u8 {
    let requested = clamp_agent_spawn_depth(requested.or(Some(definition_depth)));
    match parent_remaining {
        Some(parent_remaining) => requested.min(parent_remaining.saturating_sub(1)),
        None => requested,
    }
}

pub(crate) fn spawn_child_agent_background(
    context: AgentToolContext,
    agent: AgentDefinition,
    prompt: String,
) -> Result<AgentRunRecord> {
    if prompt.trim().is_empty() {
        return Err(Error::Message("agent message is empty".to_string()));
    }
    let id = Uuid::now_v7().to_string();
    let task_name = default_task_name(&agent.name, &id);
    let role = AgentInvocationRole::Subagent;
    let background = true;
    let spawn_depth_remaining = child_spawn_depth_remaining(&context, &agent, None);
    let child_model = child_model_from(&context, &agent, None);
    let metadata = child_agent_metadata(ChildAgentMetadataInput {
        id: &id,
        task_name: &task_name,
        agent: &agent,
        parent_session_id: &context.parent_session_id,
        role,
        task: &prompt,
        background,
        fork_context: false,
        spawn_depth_remaining,
        context: Some(&context),
        parent_tool_call_id: None,
    });
    let child_session = context.state.store().create_child_session_with_metadata(
        &context.parent_session_id,
        &context.cwd,
        "agent",
        &child_model,
        &context.model_provider,
        Some(metadata.clone()),
    )?;
    context.state.store().upsert_agent_edge(
        &context.parent_session_id,
        &child_session,
        AgentEdgeStatus::Open,
        Some(metadata),
    )?;
    let record = AgentRunRecord {
        id: id.clone(),
        task_name: Some(task_name.clone()),
        agent_name: agent.name.clone(),
        task: prompt.clone(),
        parent_session_id: context.parent_session_id.clone(),
        child_session_id: Some(child_session.clone()),
        role,
        background,
        status: AgentRunStatus::Running,
        edge_status: Some(AgentEdgeStatus::Open),
        started_at_ms: now_ms(),
        ended_at_ms: None,
        outcome: None,
        final_answer: None,
        error: None,
        effective_max_spawn_depth: Some(spawn_depth_remaining),
    };
    let (control_handle, control_receivers) = ControlHandle::new();
    {
        let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
        runs.insert(
            id.clone(),
            AgentRunState {
                record: record.clone(),
                control: Some(control_handle),
            },
        );
    }
    append_parent_agent_start_notification(
        context.state.store(),
        &context.parent_session_id,
        &record,
    )?;
    let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
    let child = ChildRun {
        id,
        context,
        agent,
        prompt,
        task_name,
        model_override: None,
        fork_context: false,
        fork_turns: None,
        max_turns: None,
        spawn_depth_remaining,
        role,
        background,
        parent_tool_call_id: None,
        existing_child_session: Some(child_session),
        previous_messages_override: Some(Vec::new()),
        control_receivers,
        abort: AbortSignal::new(abort_rx),
    };
    tokio::spawn(async move {
        let _ = run_child_agent(child).await;
    });
    Ok(record)
}

pub(crate) struct ChildRun {
    pub(crate) id: String,
    pub(crate) context: AgentToolContext,
    pub(crate) agent: AgentDefinition,
    pub(crate) prompt: String,
    pub(crate) task_name: String,
    pub(crate) model_override: Option<String>,
    pub(crate) fork_context: bool,
    pub(crate) fork_turns: Option<String>,
    pub(crate) max_turns: Option<usize>,
    pub(crate) spawn_depth_remaining: u8,
    pub(crate) role: AgentInvocationRole,
    pub(crate) background: bool,
    pub(crate) parent_tool_call_id: Option<String>,
    pub(crate) existing_child_session: Option<String>,
    pub(crate) previous_messages_override: Option<Vec<Message>>,
    pub(crate) control_receivers: psychevo_agent_core::ControlReceivers,
    pub(crate) abort: AbortSignal,
}

pub(crate) async fn maybe_preflight_child_compaction(
    context: &AgentToolContext,
    child_session: &str,
    child_model: &str,
) -> Result<()> {
    let _ = compact_session(CompactSessionOptions {
        state: context.state.clone(),
        cwd: context.cwd.clone(),
        session: child_session.to_string(),
        config_path: context.config_path.clone(),
        model: Some(format!("{}/{}", context.model_provider, child_model)),
        reasoning_effort: context.reasoning_effort.clone(),
        inherited_env: Some(context.env.clone()),
        reason: CompactionReason::AutoThreshold,
        instructions: None,
        force: false,
    })
    .await?;
    Ok(())
}
