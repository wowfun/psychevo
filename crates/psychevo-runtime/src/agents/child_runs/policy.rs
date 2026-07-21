pub(crate) async fn run_child_agent(child: ChildRun) -> Result<AgentRunRecord> {
    if child.abort.aborted() {
        if let Some(child_session) = child.existing_child_session.as_deref() {
            update_run_child_session(&child.id, child_session);
            let _ = child
                .context
                .state
                .store()
                .set_agent_edge_status(child_session, AgentEdgeStatus::Closed);
        }
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
                &child.context.cwd,
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
                    team_member_id: child.team_member_id.as_deref(),
                    context: Some(&child.context),
                    parent_tool_call_id: child.parent_tool_call_id.as_deref(),
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
                team_member_id: child.team_member_id.as_deref(),
                context: Some(&child.context),
                parent_tool_call_id: child.parent_tool_call_id.as_deref(),
            })),
        )?;
    }
    let hook_runtime = child_hook_runtime(&child)?;
    if let Some(runtime) = &hook_runtime {
        let outcome = runtime.run_subagent_start(&json!({
            "id": child.id.clone(),
            "agent": child.agent.name.clone(),
            "child_session_id": child_session.clone(),
            "parent_session_id": child.context.parent_session_id.clone(),
        }));
        if let Some(reason) = outcome.stop_reason {
            update_run_failed(&child.id, &reason);
            let _ = child
                .context
                .state
                .store()
                .set_agent_edge_status(&child_session, AgentEdgeStatus::Closed);
            return Err(Error::Message(reason));
        }
    }

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
    let sandbox_grants = child
        .context
        .state
        .filesystem_grants(&child.context.parent_session_id);
    let permission_mode =
        narrow_permission_mode_for_agent(child.context.permission_mode, Some(&child.agent));
    let permission_runtime = PermissionRuntime::new(
        child.context.cwd.clone(),
        child.context.cwd.join(".psychevo"),
        child.context.permission_config.clone(),
        permission_mode,
        child.context.approval_mode,
        child.context.approval_handler.clone(),
        None,
    );
    let permission_runtime = permission_runtime
        .with_protected_config_paths(child.context.protected_config_paths.clone());
    let permission_runtime = match hook_runtime.clone() {
        Some(runtime) => permission_runtime.with_hook_runtime(runtime),
        None => permission_runtime,
    };
    let permission_runtime = permission_runtime
        .with_sandbox(child.context.sandbox_policy.clone(), sandbox_grants.clone());
    let mut mcp_manager = crate::mcp::McpConnectionManager::default();
    let mcp_snapshot = mcp_manager
        .snapshot(
            &child.context.extension_inputs.mcp_servers,
            &child.context.cwd,
            Some(&permission_runtime),
        )
        .await;
    if !mcp_snapshot.required_failures.is_empty() {
        return Err(Error::Message(format!(
            "required MCP server unavailable: {}",
            mcp_snapshot.required_failures.join("; ")
        )));
    }
    emit_child_warning_events(&child, &child_session, &mcp_snapshot.warnings);
    let mut extension_tools = mcp_snapshot
        .tools
        .into_iter()
        .map(|tool| {
            let source_id = crate::mcp::mcp_tool_source_id(tool.name());
            let source_kind = crate::mcp::mcp_tool_source_kind(tool.name());
            RuntimeTool::with_source(tool, source_id, source_kind)
        })
        .collect::<Vec<_>>();
    extension_tools.extend(child.context.extension_inputs.runtime_tools.clone());
    let tool_surface = assemble_tool_surface_with_warnings(ToolSurfaceAssembly {
        cwd: child.context.cwd.clone(),
        task_id: child_session.clone(),
        mode: child.context.mode,
        lsp: child.context.lsp.clone(),
        allow_login_shell: child.context.permission_config.allow_login_shell,
        stream_events: child.context.stream_events.clone(),
        workspace_mutations: child.context.workspace_mutations.clone(),
        env: child.context.env.clone(),
        path_prefixes: child.context.path_prefixes.clone(),
        sandbox_policy: child.context.sandbox_policy.clone(),
        sandbox_grants: sandbox_grants.clone(),
        home: Some(child.context.home.clone()),
        image_input_enabled: child.context.image_input_enabled,
        image_generation: child.context.image_generation.clone(),
        web_search: child.context.web_search.clone(),
        tool_selection: child.context.tool_selection.clone(),
        custom_toolsets: child.context.custom_toolsets.clone(),
        contributed_toolsets: child.context.extension_inputs.toolsets.clone(),
        clarify: ClarifyToolSurface::Disabled,
        skills: None,
        extension_tools,
        agents: Some(child_agent_tool_context),
    });
    emit_child_warning_events(&child, &child_session, &tool_surface.warnings);
    let mut tools = tool_surface.tools;
    tools = apply_agent_tool_policy(tools, Some(&child.agent), child.context.mode);
    tools = permission_runtime.wrap_tools(tools);
    if let Some(runtime) = hook_runtime.clone() {
        tools = apply_hook_runtime(tools, runtime);
    }
    let effective_tool_names = effective_tool_names(&tools);
    let tool_search_config = &child.context.tool_selection.tool_search;
    let tool_search_options = if tool_search_config.enabled {
        psychevo_agent_core::ToolSearchOptions {
            enabled: true,
            default_limit: tool_search_config.default_limit,
            max_limit: tool_search_config.max_limit,
        }
    } else {
        psychevo_agent_core::ToolSearchOptions::disabled()
    };
    let tool_declarations_hash = tool_declarations_hash_with_search(&tools, tool_search_options);
    let selected_agent = SelectedAgent {
        name: child.agent.name.clone(),
        source: child.agent.source.as_str().to_string(),
        path: child.agent.file_path.clone(),
    };
    let prompt_assembly = assemble_child_prompt_prefix(
        child.context.mode,
        &child.context.cwd,
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
        "cwd": child.context.cwd.display().to_string(),
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
        "cwd": prefix_metadata.get("cwd").cloned().unwrap_or_default(),
    });
    let runtime_time_context = RuntimeTimeContext::local_now();
    let turn_prompt_instructions = vec![turn_runtime_time_instruction(
        &runtime_time_context,
        &child.context.model_metadata.capabilities,
        0,
    )];
    let prompt_context_evidence = context_evidence_for_request(
        &prompt_assembly.prompt_instructions,
        &turn_prompt_instructions,
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
        turn_prompt_instructions,
        previous_messages,
        context_messages: Vec::new(),
        prefix_contextual_user_messages: prompt_assembly.prefix_contextual_user_messages,
        turn_contextual_user_messages: Vec::new(),
        prompt_messages: vec![user_text_message(child.prompt.clone())],
        tools,
        tool_search: tool_search_options,
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
        session_id: child_session.clone(),
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
            let _ = child
                .context
                .state
                .store()
                .set_agent_edge_status(&child_session, AgentEdgeStatus::Closed);
            if let Some(runtime) = &hook_runtime {
                let _ = runtime.run_subagent_stop(&json!({
                    "id": child.id.clone(),
                    "agent": child.agent.name.clone(),
                    "outcome": "failed",
                    "error": err.to_string(),
                }));
            }
            return Err(err.into());
        }
    };
    let final_answer = completion
        .messages
        .iter()
        .rev()
        .find_map(assistant_text)
        .unwrap_or_default();
    if let Some(runtime) = &hook_runtime {
        let stop = runtime.run_subagent_stop(&json!({
            "id": child.id.clone(),
            "agent": child.agent.name.clone(),
            "outcome": completion.outcome.as_str(),
            "final_answer": final_answer.clone(),
        }));
        if let Some(reason) = stop.block_reason {
            update_run_failed(&child.id, &reason);
            let _ = child
                .context
                .state
                .store()
                .set_agent_edge_status(&child_session, AgentEdgeStatus::Closed);
            return Err(Error::Message(reason));
        }
    }
    let record = update_run_completed(&child.id, completion.outcome, final_answer.clone());
    let _ = child
        .context
        .state
        .store()
        .set_agent_edge_status(&child_session, AgentEdgeStatus::Closed);
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
    stream(RunStreamEvent::value(json!({
        "type": "agent_session_start",
        "tool_call_id": child.parent_tool_call_id.clone(),
        "agent_id": child.id.clone(),
        "agent_name": child.agent.name.clone(),
        "agent_description": child.agent.description.clone(),
        "agent_type": child.agent.name.clone(),
        "agent_path": agent_path(&child.task_name),
        "team_run_id": child.context.active_team.as_ref().map(|team| team.team_run_id.clone()),
        "mission_run_id": child.context.active_team.as_ref().and_then(|team| team.mission_run_id.clone()),
        "team_name": child.context.active_team.as_ref().map(|team| team.team_name.clone()),
        "team_member_id": child.team_member_id.clone(),
        "task_name": child.task_name.clone(),
        "message": child.prompt.clone(),
        "task": child.prompt.clone(),
        "parent_session_id": child.context.parent_session_id.clone(),
        "parent_thread_id": child.context.parent_session_id.clone(),
        "child_session_id": child_session_id,
        "child_thread_id": child_session_id,
        "background": child.background,
        "role": invocation_role_str(child.role),
        "effective_max_spawn_depth": child.spawn_depth_remaining,
    })));
}

fn emit_child_warning_events(child: &ChildRun, child_session_id: &str, warnings: &[RunWarning]) {
    let Some(stream) = &child.context.stream_events else {
        return;
    };
    for warning in warnings {
        stream(RunStreamEvent::scoped(
            child_session_id.to_string(),
            RunStreamEvent::value(crate::run::warning_event(warning)),
        ));
    }
}

struct ExternalAgentSessionStart<'a> {
    context: &'a AgentToolContext,
    agent: &'a AgentDefinition,
    id: &'a str,
    task_name: &'a str,
    task: &'a str,
    tool_call_id: &'a str,
    child_session_id: &'a str,
    spawn_depth_remaining: u8,
    team_member_id: Option<&'a str>,
    runtime_ref: Option<&'a str>,
}

fn emit_external_agent_session_start(event: ExternalAgentSessionStart<'_>) {
    let Some(stream) = &event.context.stream_events else {
        return;
    };
    let optional_contributions_omitted = optional_external_agent_contributions(event.agent);
    stream(RunStreamEvent::value(json!({
        "type": "agent_session_start",
        "tool_call_id": event.tool_call_id,
        "agent_id": event.id,
        "agent_name": event.agent.name.clone(),
        "agent_description": event.agent.description.clone(),
        "agent_type": event.agent.name.clone(),
        "agent_path": agent_path(event.task_name),
        "task_name": event.task_name,
        "message": event.task,
        "task": event.task,
        "parent_session_id": event.context.parent_session_id.clone(),
        "parent_thread_id": event.context.parent_session_id.clone(),
        "child_session_id": event.child_session_id,
        "child_thread_id": event.child_session_id,
        "background": false,
        "role": invocation_role_str(AgentInvocationRole::Subagent),
        "backend_ref": event.agent.backend.as_ref().map(|backend| backend.name.clone()),
        "runtime_ref": event.runtime_ref,
        "optional_contributions_omitted": optional_contributions_omitted,
        "team_run_id": event.context.active_team.as_ref().map(|team| team.team_run_id.clone()),
        "mission_run_id": event.context.active_team.as_ref().and_then(|team| team.mission_run_id.clone()),
        "team_name": event.context.active_team.as_ref().map(|team| team.team_name.clone()),
        "team_member_id": event.team_member_id,
        "effective_max_spawn_depth": event.spawn_depth_remaining,
    })));
}

fn child_hook_runtime(child: &ChildRun) -> Result<Option<crate::hooks::HookRuntime>> {
    let config = match child_hook_runtime_config(&child.context) {
        Ok(config) => config,
        Err(_err) if child.context.env.is_empty() && child.context.config_path.is_none() => {
            crate::hooks::HookRuntimeConfig::default()
        }
        Err(err) => return Err(err),
    };
    Ok(build_hook_runtime(
        Some(&child.agent),
        child.context.extension_inputs.hook_sources.clone(),
        config,
        &child.context.cwd,
    ))
}

fn child_hook_runtime_config(
    context: &AgentToolContext,
) -> Result<crate::hooks::HookRuntimeConfig> {
    let options = crate::types::RunOptions {
        state: context.state.clone(),
        cwd: context.cwd.clone(),
        snapshot_root: None,
        session: None,
        continue_latest: false,
        prompt: String::new(),
        image_inputs: Vec::new(),
        extract_prompt_image_sources: false,
        prompt_display: None,
        max_context_messages: None,
        config_path: context.config_path.clone(),
        project_context_override: Some(context.project_context_mode),
        sandbox_override: None,
        model: None,
        reasoning_effort: context.reasoning_effort.clone(),
        runtime_ref: None,
        runtime_session_id: None,
        runtime_options: BTreeMap::new(),
        include_reasoning: false,
        mode: context.mode,
        permission_mode: Some(context.permission_mode),
        approval_mode: Some(context.approval_mode),
        approval_handler: context.approval_handler.clone(),
        clarify_enabled: false,
        inherited_env: Some(context.env.clone()),
        agent: None,
        external_agent_delegate: None,
        no_agents: false,
        no_skills: true,
        selected_capability_roots: Vec::new(),
        skill_inputs: Vec::new(),
        mcp_servers: Vec::new(),
        workspace_mutations: context.workspace_mutations.clone(),
        runtime_tools: Vec::new(),
    };
    crate::hooks::hook_runtime_config_from_options(&options, &context.cwd)
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

pub(crate) fn agent_path(task_name: &str) -> String {
    format!("/root/{task_name}")
}

pub(crate) fn sanitize_task_name(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_' {
            out.push(ch);
        } else if ch.is_ascii_uppercase() {
            out.push(ch.to_ascii_lowercase());
        } else if (ch.is_whitespace() || ch == '-') && !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        "agent_task".to_string()
    } else {
        out
    }
}

pub(crate) fn default_task_name(agent_name: &str, id: &str) -> String {
    let suffix = id.split('-').next().unwrap_or(id);
    sanitize_task_name(&format!("{agent_name}_{suffix}"))
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
    pub(crate) team_member_id: Option<&'a str>,
    pub(crate) context: Option<&'a AgentToolContext>,
    pub(crate) parent_tool_call_id: Option<&'a str>,
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
    if let Some(team) = input
        .context
        .and_then(|context| context.active_team.as_ref())
    {
        object.insert(
            "teamRunId".to_string(),
            Value::String(team.team_run_id.clone()),
        );
        object.insert(
            "teamName".to_string(),
            Value::String(team.team_name.clone()),
        );
        if let Some(mission_run_id) = &team.mission_run_id {
            object.insert(
                "missionRunId".to_string(),
                Value::String(mission_run_id.clone()),
            );
        }
        object.insert(
            "team".to_string(),
            json!({
                "team_run_id": team.team_run_id,
                "mission_run_id": team.mission_run_id,
                "team_name": team.team_name,
                "mission_goal": team.mission_goal,
                "leader_agent_name": team.leader_agent_name,
                "max_parallel_agents": team.max_parallel_agents,
            }),
        );
    }
    let team_member = input.team_member_id.and_then(|member_id| {
        input
            .context
            .and_then(|context| context.active_team.as_ref())
            .and_then(|team| team.member(member_id))
    });
    if let Some(team_member_id) = input.team_member_id {
        object.insert(
            "teamMemberId".to_string(),
            Value::String(team_member_id.to_string()),
        );
    }
    if let Some(runtime_ref) = team_member.and_then(|member| member.runtime_ref.as_ref()) {
        object.insert("runtimeRef".to_string(), Value::String(runtime_ref.clone()));
    }
    if let Some(member) = team_member
        && !member.runtime_options.is_empty()
    {
        object.insert("runtimeOptions".to_string(), json!(member.runtime_options));
    }
    if let Some(revision) = team_member.and_then(|member| member.runtime_profile_revision) {
        object.insert("runtimeProfileRevision".to_string(), Value::from(revision));
    }
    let optional_contributions_omitted =
        if external_agent_runtime_ref(input.agent, team_member).is_some() {
            optional_external_agent_contributions(input.agent)
        } else {
            Vec::new()
        };
    if !optional_contributions_omitted.is_empty() {
        object.insert(
            "optionalContributionsOmitted".to_string(),
            json!(optional_contributions_omitted),
        );
        object.insert(
            "contributionDiagnostics".to_string(),
            json!(optional_contributions_omitted
                .iter()
                .map(|name| format!(
                    "optional Agent Definition {name} contribution omitted by external runtime delegate"
                ))
                .collect::<Vec<_>>()),
        );
    }
    object.insert(
        "agent".to_string(),
        json!({
            "id": input.id,
            "task_name": input.task_name,
            "name": input.agent.name.clone(),
            "agent_type": input.agent.name.clone(),
            "agent_path": agent_path(input.task_name),
            "team_run_id": input.context.and_then(|context| context.active_team.as_ref()).map(|team| team.team_run_id.clone()),
            "mission_run_id": input.context.and_then(|context| context.active_team.as_ref()).and_then(|team| team.mission_run_id.clone()),
            "team_name": input.context.and_then(|context| context.active_team.as_ref()).map(|team| team.team_name.clone()),
            "team_member_id": input.team_member_id,
            "runtime_ref": team_member.and_then(|member| member.runtime_ref.clone()),
            "runtime_options": team_member.map(|member| member.runtime_options.clone()).unwrap_or_default(),
            "runtime_profile_revision": team_member.and_then(|member| member.runtime_profile_revision),
            "optional_contributions_omitted": optional_contributions_omitted,
            "source": input.agent.source.as_str(),
            "path": input.agent.file_path.clone(),
            "parent_session_id": input.parent_session_id,
            "parent_thread_id": input.parent_session_id,
            "role": invocation_role_str(input.role),
            "message": input.task,
            "task": input.task,
            "background": input.background,
            "fork_context": input.fork_context,
            "effective_max_spawn_depth": input.spawn_depth_remaining,
            "max_spawn_depth": input.spawn_depth_remaining,
            "parent_tool_call_id": input.parent_tool_call_id,
        }),
    );
    Value::Object(object)
}
