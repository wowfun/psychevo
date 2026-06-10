#[allow(unused_imports)]
pub(crate) use super::*;

#[derive(Debug, Deserialize)]
pub(crate) struct AgentToolArgs {
    #[serde(default)]
    pub(crate) agent_type: Option<String>,
    #[serde(default)]
    pub(crate) name: Option<String>,
    pub(crate) prompt: String,
    #[serde(default)]
    pub(crate) task_name: Option<String>,
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
    args: AgentToolArgs,
    tool_call_id: String,
    abort: AbortSignal,
) -> Result<ToolOutput> {
    if args.prompt.trim().is_empty() {
        return Err(Error::Message("Agent prompt is empty".to_string()));
    }
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
    let id = Uuid::now_v7().to_string();
    let task_name = args
        .task_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(sanitize_task_name)
        .unwrap_or_else(|| default_task_name(&agent.name, &id));
    let spawn_depth_remaining = child_spawn_depth_remaining(&context, &agent, args.max_spawn_depth);
    let background =
        args.fork_context || agent.background.unwrap_or(false) || args.background.unwrap_or(false);
    let role = if args.fork_context {
        AgentInvocationRole::Fork
    } else {
        AgentInvocationRole::Subagent
    };
    let record = AgentRunRecord {
        id: id.clone(),
        task_name: Some(task_name.clone()),
        agent_name: agent.name.clone(),
        task: args.prompt.clone(),
        parent_session_id: context.parent_session_id.clone(),
        child_session_id: None,
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
    let child = ChildRun {
        id: id.clone(),
        context,
        agent,
        prompt: args.prompt,
        task_name,
        model_override: args.model,
        fork_context: args.fork_context,
        fork_turns: args.fork_turns,
        max_turns: args.max_turns,
        spawn_depth_remaining,
        role,
        background,
        parent_tool_call_id: Some(tool_call_id),
        existing_child_session: None,
        previous_messages_override: None,
        control_receivers,
        abort,
    };

    if background {
        tokio::spawn(async move {
            let _ = run_child_agent(child).await;
        });
        let system_value = json!({
            "id": id,
            "agent_name": response_agent_name,
            "agent_description": response_agent_description,
            "task_name": response_task_name,
            "status": "running",
            "background": true,
            "effective_max_spawn_depth": spawn_depth_remaining
        });
        let model_value = subagent_summary_value(Some(&response_store), &response_record, true);
        Ok(ToolOutput::ok_with_model_content(
            system_value,
            model_content_string(&model_value),
        ))
    } else {
        let record = run_child_agent(child).await?;
        let model_value = subagent_summary_value(Some(&response_store), &record, false);
        let response_child_session_id = record.child_session_id.clone();
        let child_summary = record
            .child_session_id
            .as_deref()
            .and_then(|session_id| response_store.session_summary(session_id).ok().flatten())
            .map(|summary| agent_child_session_summary_value(&response_store, &summary));
        let system_value = json!({
            "id": record.id,
            "agent_name": record.agent_name,
            "agent_description": response_agent_description,
            "task_name": record.task_name,
            "task": record.task,
            "status": record.status.as_str(),
            "background": false,
            "session_id": response_child_session_id,
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

pub(crate) fn resolve_agent_tool_name(
    args: &AgentToolArgs,
    required_agent_names: &[String],
) -> Result<String> {
    let agent_type = normalized_optional_name(args.agent_type.as_deref());
    let name_alias = normalized_optional_name(args.name.as_deref());
    if let (Some(agent_type), Some(name_alias)) = (&agent_type, &name_alias)
        && agent_type != name_alias
    {
        return Err(Error::Config(format!(
            "Agent arguments agent_type `{agent_type}` and name `{name_alias}` conflict"
        )));
    }
    if let Some(name) = agent_type.or(name_alias) {
        return Ok(name);
    }
    match required_agent_names {
        [single] => Ok(single.clone()),
        [] => Ok("general".to_string()),
        many => Err(Error::Config(format!(
            "Agent call must set agent_type when the user mentioned multiple agents: {}",
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
        return Err(Error::Message("Agent prompt is empty".to_string()));
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
    });
    let child_session = context.state.store().create_child_session_with_metadata(
        &context.parent_session_id,
        &context.workdir,
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
        workdir: context.workdir.clone(),
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

pub(crate) async fn run_child_agent(child: ChildRun) -> Result<AgentRunRecord> {
    if child.abort.aborted() {
        update_run_failed(&child.id, "parent invocation aborted");
        return Err(Error::Message("parent invocation aborted".to_string()));
    }
    let child_model = child_model(&child);
    let child_session = if let Some(child_session) = child.existing_child_session.clone() {
        child.context.state.store().resume_session(&child_session)?;
        child
            .context
            .state
            .store()
            .set_agent_edge_status(&child_session, AgentEdgeStatus::Open)?;
        child_session
    } else {
        child
            .context
            .state
            .store()
            .create_child_session_with_metadata(
                &child.context.parent_session_id,
                &child.context.workdir,
                "agent",
                &child_model,
                &child.context.model_provider,
                Some(child_agent_metadata(ChildAgentMetadataInput {
                    id: &child.id,
                    task_name: &child.task_name,
                    agent: &child.agent,
                    parent_session_id: &child.context.parent_session_id,
                    role: child.role,
                    task: &child.prompt,
                    background: child.background,
                    fork_context: child.fork_context,
                    spawn_depth_remaining: child.spawn_depth_remaining,
                    context: Some(&child.context),
                })),
            )?
    };
    update_run_child_session(&child.id, &child_session);
    emit_agent_session_start(&child, &child_session);
    if child.existing_child_session.is_none() {
        child.context.state.store().upsert_agent_edge(
            &child.context.parent_session_id,
            &child_session,
            AgentEdgeStatus::Open,
            Some(child_agent_metadata(ChildAgentMetadataInput {
                id: &child.id,
                task_name: &child.task_name,
                agent: &child.agent,
                parent_session_id: &child.context.parent_session_id,
                role: child.role,
                task: &child.prompt,
                background: child.background,
                fork_context: child.fork_context,
                spawn_depth_remaining: child.spawn_depth_remaining,
                context: Some(&child.context),
            })),
        )?;
    }
    run_agent_hook_event(
        Some(&child.agent),
        "SubagentStart",
        &child.context.workdir,
        json!({
            "id": child.id.clone(),
            "child_session_id": child_session.clone(),
            "parent_session_id": child.context.parent_session_id.clone(),
        }),
    );

    if child.existing_child_session.is_some() && child.previous_messages_override.is_none() {
        maybe_preflight_child_compaction(&child.context, &child_session, &child_model).await?;
    }
    let previous_messages = match child.previous_messages_override.clone() {
        Some(messages) => messages,
        None if child.existing_child_session.is_some() => {
            load_projected_messages(child.context.state.store(), &child_session, None)?
        }
        None => fork_messages(
            &child.context.parent_context_snapshot,
            child.fork_context,
            child.fork_turns.as_deref(),
        ),
    };
    let mut child_agent_tool_context = child.context.clone();
    child_agent_tool_context.parent_session_id = child_session.clone();
    child_agent_tool_context.parent_context_snapshot = previous_messages.clone();
    child_agent_tool_context.required_agent_names = Vec::new();
    child_agent_tool_context.spawn_depth_remaining = Some(child.spawn_depth_remaining);
    let sandbox_grants = crate::sandbox::SandboxWriteGrants::default();
    let tool_surface = assemble_tool_surface_with_warnings(ToolSurfaceAssembly {
        workdir: child.context.workdir.clone(),
        task_id: child_session.clone(),
        mode: child.context.mode,
        lsp: child.context.lsp.clone(),
        allow_login_shell: child.context.permission_config.allow_login_shell,
        stream_events: child.context.stream_events.clone(),
        env: child.context.env.clone(),
        path_prefixes: child.context.path_prefixes.clone(),
        sandbox_policy: child.context.sandbox_policy.clone(),
        sandbox_grants: sandbox_grants.clone(),
        tool_selection: child.context.tool_selection.clone(),
        custom_toolsets: child.context.custom_toolsets.clone(),
        clarify: ClarifyToolSurface::Disabled,
        skills: None,
        extension_tools: Vec::new(),
        agents: Some(child_agent_tool_context),
    });
    let mut tools = tool_surface.tools;
    tools = apply_agent_tool_policy(tools, Some(&child.agent), child.context.mode);
    tools = apply_agent_hooks(tools, Some(&child.agent), &child.context.workdir);
    let permission_mode =
        narrow_permission_mode_for_agent(child.context.permission_mode, Some(&child.agent));
    let permission_runtime = PermissionRuntime::new(
        child.context.workdir.clone(),
        child.context.workdir.join(".psychevo"),
        child.context.permission_config.clone(),
        permission_mode,
        child.context.approval_mode,
        child.context.approval_handler.clone(),
        None,
    );
    let permission_runtime =
        permission_runtime.with_sandbox(child.context.sandbox_policy.clone(), sandbox_grants);
    tools = permission_runtime.wrap_tools(tools);
    let effective_tool_names = effective_tool_names(&tools);
    let tool_declarations_hash = tool_declarations_hash(&tools);
    let selected_agent = SelectedAgent {
        name: child.agent.name.clone(),
        source: child.agent.source.as_str().to_string(),
        path: child.agent.file_path.clone(),
    };
    let prompt_assembly = assemble_child_prompt_prefix(
        child.context.mode,
        &child.context.workdir,
        &child.agent,
        &child.context.model_metadata.capabilities,
        !tools.is_empty(),
    );
    let prefix_metadata = json!({
        "mode": child.context.mode.as_str(),
        "permission_mode": permission_mode.as_str(),
        "approval_mode": child.context.approval_mode.as_str(),
        "selected_agent": selected_agent.clone(),
        "agent_role": invocation_role_str(child.role),
        "parent_session_id": child.context.parent_session_id.clone(),
        "effective_tools": effective_tool_names,
        "agent_catalog_visible": false,
        "visible_agents": [],
        "skill_catalog_visible": false,
        "project_instructions_visible": false,
        "project_instructions_role": serde_json::Value::Null,
        "project_context": {
            "instructions": child.context.project_context_mode.as_str(),
        },
        "workdir": child.context.workdir.display().to_string(),
    });
    let prefix_record = prompt_prefix_record(PromptPrefixRecordInput {
        session_id: &child_session,
        provider: &child.context.model_provider,
        model: &child_model,
        prefix_hash: prompt_assembly.prefix_hash.clone(),
        tool_declarations_hash,
        invalidation_reason: Some(if child.existing_child_session.is_some() {
            "child_session_resumed".to_string()
        } else {
            "new_child_session".to_string()
        }),
        slots: prompt_assembly.prefix_slots.clone(),
        metadata: Some(prefix_metadata.clone()),
    });
    let prefix_record = child
        .context
        .state
        .store()
        .upsert_session_prompt_prefix(prefix_record)?;
    let prompt_prefix_metadata = json!({
        "hash": prefix_record.prefix_hash,
        "version": prefix_record.version,
        "created_at_ms": prefix_record.created_at_ms,
        "provider": prefix_record.provider,
        "model": prefix_record.model,
        "tool_declarations_hash": prefix_record.tool_declarations_hash,
        "invalidation_reason": prefix_record.invalidation_reason,
        "effective_tools": prefix_metadata.get("effective_tools").cloned().unwrap_or_default(),
        "agent_catalog_visible": prefix_metadata.get("agent_catalog_visible").cloned().unwrap_or_default(),
        "visible_agents": prefix_metadata.get("visible_agents").cloned().unwrap_or_default(),
        "skill_catalog_visible": prefix_metadata.get("skill_catalog_visible").cloned().unwrap_or_default(),
        "project_instructions_visible": prefix_metadata.get("project_instructions_visible").cloned().unwrap_or_default(),
        "project_instructions_role": prefix_metadata.get("project_instructions_role").cloned().unwrap_or_default(),
        "project_context": prefix_metadata.get("project_context").cloned().unwrap_or_default(),
        "workdir": prefix_metadata.get("workdir").cloned().unwrap_or_default(),
    });
    let prompt_context_evidence = context_evidence_for_request(
        &prompt_assembly.prompt_instructions,
        &[],
        &prompt_assembly.prefix_contextual_user_messages,
        &[],
    );
    let mut generation_metadata = child.context.generation_metadata.clone();
    if let Some(object) = generation_metadata.as_object_mut() {
        object.insert("prompt_prefix".to_string(), prompt_prefix_metadata.clone());
    }
    let request = AgentLoopRequest {
        model_provider: child.context.model_provider.clone(),
        model: child_model,
        generation_metadata,
        prompt_instructions: prompt_assembly.prompt_instructions,
        turn_prompt_instructions: Vec::new(),
        previous_messages,
        context_messages: Vec::new(),
        prefix_contextual_user_messages: prompt_assembly.prefix_contextual_user_messages,
        turn_contextual_user_messages: Vec::new(),
        prompt_messages: vec![user_text_message(child.prompt.clone())],
        tools,
        max_turns: child
            .max_turns
            .or(child.agent.max_turns)
            .unwrap_or(SUBAGENT_DEFAULT_MAX_TURNS),
    };

    let child_stream_events = child.context.stream_events.as_ref().map(|stream| {
        let stream = Arc::clone(stream);
        let child_session_id = child_session.clone();
        Arc::new(move |event| {
            stream(RunStreamEvent::scoped(child_session_id.clone(), event));
        }) as RunStreamSink
    });
    let sink = Arc::new(PersistenceSink {
        store: child.context.state.store().clone(),
        session_id: child_session,
        prompt_snapshot: None,
        prompt_snapshot_written: Arc::new(Mutex::new(false)),
        prompt_context_evidence: Arc::new(prompt_context_evidence),
        started: Instant::now(),
        tool_elapsed_ms: Arc::new(Mutex::new(BTreeMap::new())),
        current_turn_index: Arc::new(Mutex::new(None)),
        control: SmokeControl::None,
        control_handle: None,
        events: None,
        stream_events: child_stream_events,
        trace: None,
        trace_warning_emitted: Arc::new(Mutex::new(false)),
        include_reasoning: false,
        reasoning_effort: None,
        model_metadata: child.context.model_metadata.clone(),
        context_recorder: Option::<ContextRecorder>::None,
        prompt_display: None,
        selected_agent: Some(selected_agent),
        prompt_prefix_metadata: Some(prompt_prefix_metadata),
    });
    let parent_store = child.context.state.store().clone();
    let parent_session_id = child.context.parent_session_id.clone();
    let completion = match psychevo_agent_core::run_agent_loop(
        Arc::clone(&child.context.provider),
        request,
        sink,
        child.control_receivers,
    )
    .await
    {
        Ok(completion) => completion,
        Err(err) => {
            update_run_failed(&child.id, &err.to_string());
            run_agent_hook_event(
                Some(&child.agent),
                "SubagentStop",
                &child.context.workdir,
                json!({
                    "id": child.id.clone(),
                    "outcome": "failed",
                    "error": err.to_string(),
                }),
            );
            return Err(err.into());
        }
    };
    let final_answer = completion
        .messages
        .iter()
        .rev()
        .find_map(assistant_text)
        .unwrap_or_default();
    let record = update_run_completed(&child.id, completion.outcome, final_answer.clone());
    run_agent_hook_event(
        Some(&child.agent),
        "SubagentStop",
        &child.context.workdir,
        json!({
            "id": child.id.clone(),
            "outcome": completion.outcome.as_str(),
            "final_answer": final_answer.clone(),
        }),
    );
    if child.background {
        let _ = append_parent_agent_mailbox_event(
            &parent_store,
            &parent_session_id,
            &record,
            completion.outcome.as_str(),
            &final_answer,
        );
    }
    Ok(record)
}

pub(crate) fn emit_agent_session_start(child: &ChildRun, child_session_id: &str) {
    let Some(stream) = &child.context.stream_events else {
        return;
    };
    stream(RunStreamEvent::Event(json!({
        "type": "agent_session_start",
        "tool_call_id": child.parent_tool_call_id.clone(),
        "agent_id": child.id.clone(),
        "agent_name": child.agent.name.clone(),
        "agent_description": child.agent.description.clone(),
        "task_name": child.task_name.clone(),
        "task": child.prompt.clone(),
        "parent_session_id": child.context.parent_session_id.clone(),
        "child_session_id": child_session_id,
        "background": child.background,
        "role": invocation_role_str(child.role),
        "effective_max_spawn_depth": child.spawn_depth_remaining,
    })));
}

pub(crate) fn child_model(child: &ChildRun) -> String {
    child_model_from(
        &child.context,
        &child.agent,
        child.model_override.as_deref(),
    )
}

pub(crate) fn child_model_from(
    context: &AgentToolContext,
    agent: &AgentDefinition,
    model_override: Option<&str>,
) -> String {
    model_override
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "inherit")
        .map(str::to_string)
        .or_else(|| {
            context
                .env
                .get("PSYCHEVO_SUBAGENT_MODEL")
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| agent.model.clone())
        .unwrap_or_else(|| context.model.clone())
}

pub(crate) fn invocation_role_str(role: AgentInvocationRole) -> &'static str {
    match role {
        AgentInvocationRole::Main => "main",
        AgentInvocationRole::Subagent => "child",
        AgentInvocationRole::Fork => "fork",
        AgentInvocationRole::System => "system",
    }
}

pub(crate) fn sanitize_task_name(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_' {
            out.push(ch);
        } else if ch.is_ascii_uppercase() {
            out.push(ch.to_ascii_lowercase());
        } else if ch.is_whitespace() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches(['-', '_']).to_string();
    if out.is_empty() {
        "agent-task".to_string()
    } else {
        out
    }
}

pub(crate) fn default_task_name(agent_name: &str, id: &str) -> String {
    let suffix = id.split('-').next().unwrap_or(id);
    sanitize_task_name(&format!("{agent_name}-{suffix}"))
}

pub(crate) const AGENT_NOTIFICATION_METADATA_KEY: &str = "agent_notification";

pub(crate) struct ChildAgentMetadataInput<'a> {
    pub(crate) id: &'a str,
    pub(crate) task_name: &'a str,
    pub(crate) agent: &'a AgentDefinition,
    pub(crate) parent_session_id: &'a str,
    pub(crate) role: AgentInvocationRole,
    pub(crate) task: &'a str,
    pub(crate) background: bool,
    pub(crate) fork_context: bool,
    pub(crate) spawn_depth_remaining: u8,
    pub(crate) context: Option<&'a AgentToolContext>,
}

pub(crate) fn child_agent_metadata(input: ChildAgentMetadataInput<'_>) -> Value {
    let mut object = input
        .context
        .and_then(|context| context.generation_metadata.as_object().cloned())
        .unwrap_or_default();
    if let Some(context) = input.context {
        object.insert(
            "provider_label".to_string(),
            Value::String(context.provider_label.clone()),
        );
        object.insert(
            "base_url".to_string(),
            Value::String(context.base_url.clone()),
        );
        object.insert(
            "api_key_env".to_string(),
            context
                .api_key_env
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        object.insert(
            "reasoning_effort".to_string(),
            context
                .reasoning_effort
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        object.insert(
            "mode".to_string(),
            Value::String(context.mode.as_str().to_string()),
        );
        object.insert(
            "permission_mode".to_string(),
            Value::String(context.permission_mode.as_str().to_string()),
        );
        object.insert(
            "approval_mode".to_string(),
            Value::String(context.approval_mode.as_str().to_string()),
        );
        let context_limit = context
            .context_limit
            .or_else(|| context.model_metadata.context_limit());
        if let Some(limit) = context_limit {
            object.insert("context_limit".to_string(), Value::from(limit));
        }
        object
            .entry("model_metadata".to_string())
            .or_insert_with(|| context.model_metadata.public_json());
    }
    object.insert(
        "agent".to_string(),
        json!({
            "id": input.id,
            "task_name": input.task_name,
            "name": input.agent.name.clone(),
            "source": input.agent.source.as_str(),
            "path": input.agent.file_path.clone(),
            "parent_session_id": input.parent_session_id,
            "role": invocation_role_str(input.role),
            "task": input.task,
            "background": input.background,
            "fork_context": input.fork_context,
            "effective_max_spawn_depth": input.spawn_depth_remaining,
            "max_spawn_depth": input.spawn_depth_remaining,
        }),
    );
    Value::Object(object)
}
