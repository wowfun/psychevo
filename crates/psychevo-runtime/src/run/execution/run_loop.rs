#[allow(unused_imports)]
pub(crate) use super::*;
use crate::agents::{apply_hook_runtime, build_hook_runtime};
use crate::config::resolve_title_generation_provider;

pub(crate) async fn run_live_internal(
    options: RunOptions,
    source: &str,
    continue_sources: &[&str],
    stream_events: Option<RunStreamSink>,
    control: Option<RunControl>,
    overflow_retry_attempted: bool,
) -> Result<RunResult> {
    let cwd = canonical_cwd(&options.cwd)?;
    if options.prompt.trim().is_empty() && options.image_inputs.is_empty() {
        return Err(Error::Message("prompt is empty".to_string()));
    }

    if options.no_agents && options.agent.is_some() {
        return Err(Error::Config(
            "--agent cannot be used together with no_agents".to_string(),
        ));
    }
    let loaded = load_run_config(&options, &cwd)?;
    let home = crate::config::resolve_psychevo_home(&loaded.env)?;
    let mut mcp_inputs = options.mcp_servers.clone();
    mcp_inputs.extend(loaded.config.mcp_servers.clone());
    let extension_assembly =
        crate::extensions::assemble_extensions(crate::extensions::ExtensionAssemblyInput {
            home: &home,
            cwd: &cwd,
            env: &loaded.env,
            plugin_policy: &loaded.config.plugins,
            selected_capability_roots: &options.selected_capability_roots,
            mcp_servers: mcp_inputs,
            runtime_tools: options.runtime_tools.clone(),
        });
    let extension_warnings = extension_assembly.warnings.clone();
    let project_context_mode = loaded.config.project_context.instructions;
    let project_instructions = load_project_instructions(&cwd, project_context_mode)?;
    let permission_mode = options.permission_mode.unwrap_or_default();
    let approval_mode = options.approval_mode.unwrap_or({
        match loaded.config.permissions.approvals_reviewer {
            crate::types::ApprovalsReviewer::User => crate::types::ApprovalMode::Manual,
            crate::types::ApprovalsReviewer::Smart => crate::types::ApprovalMode::Smart,
        }
    });
    let store = options.state.store().clone();
    let resumed_session_id = if let Some(session_id) = &options.session {
        Some(session_id.clone())
    } else if options.continue_latest {
        store.latest_session_for_cwd_with_sources(&cwd, continue_sources)?
    } else {
        None
    };
    let session_metadata_for_agent = resumed_session_id
        .as_deref()
        .map(|session_id| store.session_metadata(session_id))
        .transpose()?
        .flatten();
    let agents_home = resolve_agents_home(&loaded.env, &cwd)?;
    let agent_input = main_agent_input_from_sources(
        options.no_agents,
        options.agent.as_deref(),
        session_metadata_for_agent.as_ref(),
    );
    let mut agent_explicit_inputs = agent_input.iter().cloned().collect::<Vec<_>>();
    agent_explicit_inputs.extend(extension_assembly.agent_inputs.clone());
    let agent_catalog = discover_agents(&AgentDiscoveryOptions {
        home: agents_home,
        cwd: cwd.clone(),
        env: loaded.env.clone(),
        explicit_inputs: agent_explicit_inputs,
        no_agents: options.no_agents,
    })?;
    let selected_agent = match &agent_input {
        Some(input) => Some(resolve_agent_definition(
            &agent_catalog,
            input,
            &cwd,
            &loaded.env,
        )?),
        None => None,
    };
    let permission_mode =
        narrow_permission_mode_for_agent(permission_mode, selected_agent.as_ref());
    let mut resolved_options = options.clone();
    if resolved_options.model.is_none()
        && let Some(model) = selected_agent
            .as_ref()
            .and_then(|agent| agent.model.clone())
    {
        resolved_options.model = Some(model);
    }
    if resolved_options.reasoning_effort.is_none()
        && let Some(effort) = selected_agent
            .as_ref()
            .and_then(|agent| agent.effort.clone())
    {
        resolved_options.reasoning_effort = Some(effort);
    }
    let resolved = resolve_run_provider(&resolved_options, &loaded)?;
    let managed_tools = ensure_rg(&loaded.env).await?;
    let skills_home = resolve_skills_home(&loaded.env, &cwd)?;
    let mut explicit_skill_inputs = options.skill_inputs.clone();
    if let Some(agent) = &selected_agent {
        explicit_skill_inputs.extend(agent.skills.clone());
    }
    let skill_options = SkillDiscoveryOptions {
        home: skills_home.clone(),
        cwd: cwd.clone(),
        config_path: options.config_path.clone(),
        env: loaded.env.clone(),
        explicit_inputs: explicit_skill_inputs.clone(),
        additional_roots: extension_assembly.skill_inputs.clone(),
        no_skills: options.no_skills,
    };
    let skill_catalog = discover_skills(&skill_options)?;
    let selected_skills = selected_skills_for_run(
        &skill_catalog,
        &options.prompt,
        &explicit_skill_inputs,
        &cwd,
        &loaded.env,
    );
    let selected_agent_summary = selected_agent_for_result(selected_agent.as_ref());
    let selected_main_agent_metadata =
        selected_agent
            .as_ref()
            .zip(agent_input.as_deref())
            .map(|(agent, input)| {
                main_agent_metadata(input, &agent.name, agent.source, agent.file_path.as_ref())
            });
    let required_agent_catalog = if options.no_agents {
        Vec::new()
    } else {
        agent_catalog_for_selected_policy(&agent_catalog.agents, selected_agent.as_ref())
    };
    let required_agent_mentions = required_agent_mentions(&options.prompt, &required_agent_catalog);
    let skill_context_fragments = skill_context_fragments(&selected_skills, &skill_catalog)?;
    let selected_skill_context_message_count = skill_context_fragments.len();
    let session_metadata = || {
        let mut metadata = json!({
            "provider_label": resolved.display_label.clone(),
            "base_url": resolved.base_url.clone(),
            "api_key_env": resolved.api_key_env.clone(),
            "reasoning_effort": resolved.reasoning_effort.clone(),
            "context_limit": resolved.context_limit,
            "model_metadata": resolved.metadata.public_json(),
            "mode": options.mode.as_str(),
            "permission_mode": permission_mode.as_str(),
            "approval_mode": approval_mode.as_str(),
            "project_context": {
                "instructions": project_context_mode.as_str(),
            },
            "cwd": cwd.display().to_string(),
            "selected_agent": selected_agent_summary.clone(),
        });
        if let Some(main_agent) = selected_main_agent_metadata.clone()
            && let Some(object) = metadata.as_object_mut()
        {
            object.insert("main_agent".to_string(), main_agent);
        }
        metadata
    };
    let first_use_empty_visible_session = options
        .session
        .as_deref()
        .map(|session_id| first_use_empty_visible_session(&store, session_id))
        .transpose()?
        .unwrap_or(false);
    let (session_id, created_session) = if let Some(session_id) = options.session.clone() {
        store.resume_session(&session_id)?;
        (session_id, false)
    } else if options.continue_latest {
        if let Some(session_id) = resumed_session_id {
            store.resume_session(&session_id)?;
            (session_id, false)
        } else {
            (
                store.create_session_with_metadata(
                    &cwd,
                    source,
                    &resolved.model,
                    &resolved.provider,
                    Some(session_metadata()),
                )?,
                true,
            )
        }
    } else {
        (
            store.create_session_with_metadata(
                &cwd,
                source,
                &resolved.model,
                &resolved.provider,
                Some(session_metadata()),
            )?,
            true,
        )
    };
    if first_use_empty_visible_session {
        materialize_first_use_empty_session(
            &store,
            &session_id,
            &resolved.provider,
            &resolved.model,
            session_metadata(),
        )?;
    }
    let invocation_started = Instant::now();
    let trace_warning_emitted = Arc::new(Mutex::new(false));
    let invocation_id = uuid::Uuid::now_v7().to_string();
    let (trace, trace_open_warning) =
        match SessionTraceSink::open(options.state.db_path(), &session_id, invocation_id) {
            Ok(trace) => (trace, None),
            Err(err) => (None, Some(err)),
        };

    store.cleanup_reverted_messages(&session_id)?;
    maybe_preflight_compact_session(
        &options,
        &cwd,
        &session_id,
        &resolved.provider,
        &resolved.model,
        &resolved.reasoning_effort,
        &loaded.env,
    )
    .await?;
    let prompt_snapshot = options.snapshot_root.as_ref().and_then(|root| {
        SnapshotStore::new(root.clone(), cwd.clone())
            .track()
            .ok()
            .flatten()
    });
    let sandbox_profile = options.sandbox_override.as_ref().map(|sandbox| {
        json!({
            "enabled": sandbox.enabled,
            "mode": match sandbox.mode {
                crate::types::RunSandboxMode::WorkspaceWrite => "workspace-write",
                crate::types::RunSandboxMode::ReadOnly => "read-only",
            },
            "writable_roots": sandbox.writable_roots.clone(),
            "include_tmp": sandbox.include_tmp,
            "include_common_caches": sandbox.include_common_caches,
        })
    });

    let run_start = json!({
        "type": "run_start",
        "source": source,
        "session_id": session_id.clone(),
        "thread_id": session_id.clone(),
        "provider": resolved.provider.clone(),
        "model": resolved.model.clone(),
        "db": options.state.db_path().to_path_buf(),
        "cwd": cwd.clone(),
        "root": cwd.clone(),
        "base_url": resolved.base_url.clone(),
        "api_key_env": resolved.api_key_env.clone(),
        "reasoning_effort": resolved.reasoning_effort.clone(),
        "context_limit": resolved.context_limit,
        "model_metadata": resolved.metadata.public_json(),
        "mode": options.mode.as_str(),
        "permission_mode": permission_mode.as_str(),
        "approval_mode": approval_mode.as_str(),
        "permission_profile": {
            "mode": permission_mode.as_str(),
            "approval_mode": approval_mode.as_str(),
            "sandbox": sandbox_profile,
        },
        "project_context": {
            "instructions": project_context_mode.as_str(),
        },
        "resume_seed": {
            "requested_session_id": options.session,
            "continue_latest": options.continue_latest,
            "resolved_session_id": session_id.clone(),
            "created_session": created_session,
            "source": if created_session { "startup" } else { "resume" },
        },
        "selected_agent": selected_agent_summary.clone(),
        "agents_enabled": !options.no_agents,
        "agent_count": agent_catalog.agents.len(),
        "selected_skills": selected_skills.clone(),
        "selected_capability_roots": options.selected_capability_roots.clone(),
    });
    if let Some(stream) = &stream_events {
        stream(RunStreamEvent::value(run_start.clone()));
    }
    let events = Arc::new(Mutex::new(vec![run_start]));
    let mut trace_warnings = Vec::new();
    if let Some(warning) = trace_open_warning {
        trace_warnings.push(RunWarning {
            kind: "session_trace".to_string(),
            message: format!("session observability trace is unavailable: {warning}"),
            source_path: None,
            suggestion: None,
        });
    }
    let run_start_trace_payload = events
        .lock()
        .expect("event lock poisoned")
        .first()
        .cloned()
        .unwrap_or_else(|| json!({ "type": "run_start" }));
    if let Some(trace) = &trace
        && let Some(warning) = trace.enqueue_run_start(&run_start_trace_payload)
    {
        trace_warnings.push(RunWarning {
            kind: "session_trace".to_string(),
            message: format!("session observability trace is unavailable: {warning}"),
            source_path: None,
            suggestion: None,
        });
    }
    if !trace_warnings.is_empty() {
        if let Ok(mut emitted) = trace_warning_emitted.lock() {
            *emitted = true;
        }
        emit_warning_events(&trace_warnings, &events, stream_events.as_ref());
    }
    emit_warning_events(
        &project_instructions.warnings,
        &events,
        stream_events.as_ref(),
    );
    emit_warning_events(&extension_warnings, &events, stream_events.as_ref());

    let previous_messages =
        load_projected_messages(&store, &session_id, options.max_context_messages)?;
    let prompt_session_seq = store.next_message_seq(&session_id)?;
    let mailbox_context_messages = store
        .deliver_pending_agent_mailbox_events_for_prompt(&session_id, prompt_session_seq)?
        .into_iter()
        .filter(|record| record.delivered_at_ms.is_some())
        .map(|record| agent_mailbox_event_message(&record))
        .collect::<Vec<_>>();
    let provider: Arc<dyn GenerationProvider> = Arc::new(OpenAiChatProvider::new(
        resolved.base_url.clone(),
        resolved.api_key.clone(),
        resolved.provider.clone(),
    ));
    let title_resolved = resolve_title_generation_provider(&resolved_options, &loaded, &resolved)?;
    let provider_for_title: Arc<dyn GenerationProvider> = Arc::new(OpenAiChatProvider::new(
        title_resolved.base_url.clone(),
        title_resolved.api_key.clone(),
        title_resolved.provider.clone(),
    ));
    let context_recorder = ContextRecorder::default();
    let provider: Arc<dyn GenerationProvider> = Arc::new(ContextRecordingProvider::new(
        Arc::clone(&provider),
        context_recorder.clone(),
        LiveContextProfile {
            session_id: session_id.clone(),
            base_url: resolved.base_url.clone(),
            context_limit: resolved.context_limit,
            mode: options.mode,
        },
    ));
    let stream_events_after = stream_events.clone();
    let controlled_run = control.is_some();
    let (control_handle, control_receivers, clarify_control) = match control {
        Some(control) => {
            let clarify = Some(control.handle.clarify.clone());
            (control.handle.inner.clone(), control.receivers, clarify)
        }
        None => {
            let (handle, receivers) = ControlHandle::new();
            (handle, receivers, None)
        }
    };
    let mut generation_metadata = json!({
        "model_metadata": resolved.metadata.public_json(),
    });
    if let Some(effort) = &resolved.reasoning_effort
        && let Some(object) = generation_metadata.as_object_mut()
    {
        object.insert(
            "reasoning_effort".to_string(),
            serde_json::Value::String(effort.clone()),
        );
    }
    let sandbox_policy = crate::sandbox::SandboxPolicy::from_config(
        &loaded.config.sandbox,
        &cwd,
        options.mode,
        &loaded.env,
    )?;
    let sandbox_grants = crate::sandbox::SandboxWriteGrants::default();
    let agent_tools = if !options.no_agents {
        Some(AgentToolContext {
            provider: Arc::clone(&provider),
            model_provider: resolved.provider.clone(),
            model: resolved.model.clone(),
            provider_label: resolved.display_label.clone(),
            base_url: resolved.base_url.clone(),
            api_key_env: resolved.api_key_env.clone(),
            reasoning_effort: resolved.reasoning_effort.clone(),
            context_limit: resolved.context_limit,
            generation_metadata: generation_metadata.clone(),
            cwd: cwd.clone(),
            mode: options.mode,
            project_context_mode,
            permission_config: loaded.config.permissions.clone(),
            lsp: loaded.config.lsp.clone(),
            permission_mode,
            approval_mode,
            approval_handler: options.approval_handler.clone(),
            state: options.state.clone(),
            config_path: options.config_path.clone(),
            parent_session_id: session_id.clone(),
            parent_context_snapshot: previous_messages.clone(),
            catalog: agent_catalog.clone(),
            control_handle: Some(control_handle.clone()),
            stream_events: stream_events.clone(),
            model_metadata: resolved.metadata.clone(),
            env: loaded.env.clone(),
            path_prefixes: managed_tools.path_prefixes.clone(),
            sandbox_policy: sandbox_policy.clone(),
            tool_selection: loaded.config.tools.clone(),
            custom_toolsets: loaded.config.toolsets.clone(),
            extension_inputs: extension_assembly.accepted_inputs(),
            allowed_agent_names: selected_agent
                .as_ref()
                .and_then(|agent| agent.tool_policy.allowed_agents.clone()),
            denied_agent_names: selected_agent
                .as_ref()
                .map(|agent| agent.tool_policy.denied_agents.clone())
                .unwrap_or_default(),
            required_agent_names: required_agent_mentions.clone(),
            spawn_depth_remaining: None,
            external_delegate: options.external_agent_delegate.clone(),
        })
    } else {
        None
    };
    let permission_runtime = PermissionRuntime::new(
        cwd.clone(),
        cwd.join(".psychevo"),
        loaded.config.permissions.clone(),
        permission_mode,
        approval_mode,
        options.approval_handler.clone(),
        smart_approval_handler(
            Arc::clone(&provider),
            &resolved,
            &loaded.config.permissions,
            generation_metadata.clone(),
        ),
    );
    let permission_runtime =
        permission_runtime.with_sandbox(sandbox_policy.clone(), sandbox_grants.clone());
    let mcp_server_inputs = extension_assembly.registry.mcp_servers();
    let mut mcp_manager = crate::mcp::McpConnectionManager::default();
    let mcp_snapshot = mcp_manager
        .snapshot(&mcp_server_inputs, &cwd, Some(&permission_runtime))
        .await;
    if !mcp_snapshot.required_failures.is_empty() {
        return Err(Error::Message(format!(
            "required MCP server unavailable: {}",
            mcp_snapshot.required_failures.join("; ")
        )));
    }
    let mcp_generation = mcp_manager.generation();
    let mcp_snapshot_hash = mcp_snapshot.snapshot_hash.clone();
    let mcp_catalog_hash = mcp_snapshot.catalog_hash.clone();
    let mcp_accepted_servers = mcp_snapshot.accepted_servers.clone();
    let mcp_resources_available = mcp_snapshot.resources_available;
    let mcp_prompts_available = mcp_snapshot.prompts_available;
    let mcp_sampling_config = mcp_snapshot.sampling_config.clone();
    let mcp_elicitation_policy = mcp_snapshot.elicitation_policy.clone();
    let mcp_warnings = mcp_snapshot.warnings.clone();
    let mut extension_tools = mcp_snapshot
        .tools
        .into_iter()
        .map(|tool| {
            let source_id = crate::mcp::mcp_tool_source_id(tool.name());
            let source_kind = crate::mcp::mcp_tool_source_kind(tool.name());
            RuntimeTool::with_source(tool, source_id, source_kind)
        })
        .collect::<Vec<_>>();
    extension_tools.extend(extension_assembly.registry.runtime_tools().iter().cloned());
    emit_warning_events(&mcp_warnings, &events, stream_events.as_ref());
    let tool_surface = assemble_tool_surface_with_warnings(ToolSurfaceAssembly {
        cwd: cwd.clone(),
        task_id: session_id.clone(),
        mode: options.mode,
        lsp: loaded.config.lsp.clone(),
        allow_login_shell: loaded.config.permissions.allow_login_shell,
        stream_events: stream_events.clone(),
        env: loaded.env.clone(),
        path_prefixes: managed_tools.path_prefixes.clone(),
        sandbox_policy,
        sandbox_grants,
        tool_selection: loaded.config.tools.clone(),
        custom_toolsets: loaded.config.toolsets.clone(),
        contributed_toolsets: extension_assembly.toolsets.clone(),
        clarify: if options.clarify_enabled {
            ClarifyToolSurface::enabled(clarify_control, stream_events.clone())
        } else {
            ClarifyToolSurface::Disabled
        },
        skills: (!options.no_skills || !explicit_skill_inputs.is_empty()).then_some(skill_options),
        extension_tools,
        agents: agent_tools,
    });
    emit_warning_events(&tool_surface.warnings, &events, stream_events.as_ref());
    let mut contribution_projection = extension_assembly.projection.clone();
    contribution_projection.extend(tool_surface.projection.clone());
    let _contribution_fact_count = contribution_projection.facts().len();
    let _accepted_tool_count = tool_surface.accepted_tool_names.len();
    let mut tool_surface_warnings = tool_surface.warnings;
    let hook_config = crate::hooks::hook_runtime_config_from_options(&options, &cwd)?;
    let hook_runtime = build_hook_runtime(
        selected_agent.as_ref(),
        extension_assembly.hook_sources.clone(),
        hook_config,
        &cwd,
    );
    let hook_runtime_for_lifecycle = hook_runtime.clone();
    if let Some(runtime) = &hook_runtime_for_lifecycle {
        let outcome = runtime.run_session_start(&json!({
            "session_id": session_id,
            "source": if created_session { "startup" } else { "resume" },
            "cwd": cwd,
        }));
        if let Some(reason) = outcome.stop_reason {
            return Err(Error::Message(reason));
        }
    }
    let mut tools = tool_surface.tools;
    tools = apply_agent_tool_policy(tools, selected_agent.as_ref(), options.mode);
    let permission_runtime = match hook_runtime.clone() {
        Some(runtime) => permission_runtime.with_hook_runtime(runtime),
        None => permission_runtime,
    };
    tools = permission_runtime.wrap_tools(tools);
    if let Some(runtime) = hook_runtime.clone() {
        tools = apply_hook_runtime(tools, runtime);
    }
    let effective_tool_names = effective_tool_names(&tools);
    let prompt_agents = if options.no_agents {
        Vec::new()
    } else {
        agent_catalog_for_prompt(&agent_catalog.agents, selected_agent.as_ref(), &tools)
    };
    let prompt_skills = if skill_catalog_visible_for_tools(&tools) {
        skills_visible_for_prompt_with_tools_and_toolsets(
            &skill_catalog.skills,
            effective_tool_names.iter().map(String::as_str),
            tool_surface
                .accepted_toolset_names
                .iter()
                .map(String::as_str),
        )
    } else {
        Vec::new()
    };
    let prompt_project_instructions = if agent_project_instructions_enabled(selected_agent.as_ref())
    {
        project_instructions.fragments.as_slice()
    } else {
        &[]
    };
    let project_instructions_role = (!prompt_project_instructions.is_empty())
        .then(|| developer_provider_role(&resolved.metadata.capabilities).to_string());
    let tool_search_config = &loaded.config.tools.tool_search;
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
    let prefix_metadata = json!({
        "mode": options.mode.as_str(),
        "permission_mode": permission_mode.as_str(),
        "approval_mode": approval_mode.as_str(),
        "project_context": {
            "instructions": project_context_mode.as_str(),
        },
        "cwd": cwd.display().to_string(),
        "selected_agent": selected_agent_summary.clone(),
        "agents_enabled": !options.no_agents,
        "effective_tools": effective_tool_names,
        "accepted_toolsets": tool_surface.accepted_toolset_names,
        "mcp_runtime": {
            "snapshot_hash": mcp_snapshot_hash,
            "catalog_hash": mcp_catalog_hash,
            "accepted_servers": mcp_accepted_servers,
            "resources_available": mcp_resources_available,
            "prompts_available": mcp_prompts_available,
            "generation": mcp_generation,
            "sampling": mcp_sampling_config,
            "elicitation": mcp_elicitation_policy,
        },
        "agent_catalog_visible": !prompt_agents.is_empty(),
        "visible_agents": prompt_agents.iter().map(|agent| agent.name.clone()).collect::<Vec<_>>(),
        "selected_skills": selected_skills.clone(),
        "skill_catalog_visible": !prompt_skills.is_empty(),
        "project_instructions_visible": !prompt_project_instructions.is_empty(),
        "project_instructions_role": project_instructions_role,
    });
    let stored_prefix = store.load_session_prompt_prefix(&session_id)?;
    let invalidation_reason = stored_prefix.as_ref().and_then(|record| {
        prompt_prefix_invalidation_reason(
            record,
            &resolved.provider,
            &resolved.model,
            options.mode,
            selected_agent_summary.as_ref(),
            &tool_declarations_hash,
            &prefix_metadata,
        )
    });
    let needs_prefix_rebuild =
        created_session || stored_prefix.is_none() || invalidation_reason.is_some();
    let (prompt_assembly, prompt_prefix_record) = if needs_prefix_rebuild {
        let assembly = assemble_main_prompt_prefix(MainPromptPrefixInput {
            mode: options.mode,
            cwd: &cwd,
            selected_agent: selected_agent.as_ref(),
            agents: &prompt_agents,
            skills: &prompt_skills,
            project_instruction_fragments: prompt_project_instructions,
            capabilities: &resolved.metadata.capabilities,
            tools_available: !tools.is_empty(),
        });
        let reason = if created_session {
            "new_session".to_string()
        } else {
            invalidation_reason.unwrap_or_else(|| "lazy_create".to_string())
        };
        let record = prompt_prefix_record(PromptPrefixRecordInput {
            session_id: &session_id,
            provider: &resolved.provider,
            model: &resolved.model,
            prefix_hash: assembly.prefix_hash.clone(),
            tool_declarations_hash: tool_declarations_hash.clone(),
            invalidation_reason: Some(reason),
            slots: assembly.prefix_slots.clone(),
            metadata: Some(prefix_metadata.clone()),
        });
        let record = store.upsert_session_prompt_prefix(record)?;
        (assembly, record)
    } else {
        let record = stored_prefix.expect("checked above");
        (assembly_from_prefix_record(&record), record)
    };
    let prefix_notice = take_prompt_prefix_notice(&store, &session_id)?;
    let mut turn_prompt_instructions = Vec::new();
    if let Some(notice) = prefix_notice.as_deref()
        && let Some(instruction) =
            turn_prefix_notice_instruction(notice, &resolved.metadata.capabilities, 0)
    {
        turn_prompt_instructions.push(instruction);
    }
    if let Some(instruction) = turn_required_agent_instruction(
        &required_agent_mentions,
        &resolved.metadata.capabilities,
        turn_prompt_instructions.len(),
    ) {
        turn_prompt_instructions.push(instruction);
    }
    let mut turn_contextual_user_messages =
        skill_contextual_user_messages(&skill_context_fragments);
    let mut hook_context_message_count = 0usize;
    if let Some(runtime) = &hook_runtime_for_lifecycle {
        let prompt_hook = runtime.run_user_prompt_submit(&json!({
            "session_id": session_id,
            "prompt": options.prompt,
            "cwd": cwd,
        }));
        if let Some(reason) = prompt_hook.block_reason {
            return Err(Error::Message(reason));
        }
        let hook_context_messages = hook_contextual_user_messages(&prompt_hook.context);
        hook_context_message_count = hook_context_messages.len();
        turn_contextual_user_messages.extend(hook_context_messages);
    }
    let prompt_context_evidence = context_evidence_for_request(
        &prompt_assembly.prompt_instructions,
        &turn_prompt_instructions,
        &prompt_assembly.prefix_contextual_user_messages,
        &skill_context_fragments,
    );
    let prompt_prefix_metadata = json!({
        "hash": prompt_prefix_record.prefix_hash,
        "version": prompt_prefix_record.version,
        "created_at_ms": prompt_prefix_record.created_at_ms,
        "provider": prompt_prefix_record.provider,
        "model": prompt_prefix_record.model,
        "tool_declarations_hash": prompt_prefix_record.tool_declarations_hash,
        "invalidation_reason": prompt_prefix_record.invalidation_reason,
        "effective_tools": prefix_metadata.get("effective_tools").cloned().unwrap_or_default(),
        "agent_catalog_visible": prefix_metadata.get("agent_catalog_visible").cloned().unwrap_or_default(),
        "visible_agents": prefix_metadata.get("visible_agents").cloned().unwrap_or_default(),
        "skill_catalog_visible": prefix_metadata.get("skill_catalog_visible").cloned().unwrap_or_default(),
        "project_instructions_visible": prefix_metadata.get("project_instructions_visible").cloned().unwrap_or_default(),
        "project_instructions_role": prefix_metadata.get("project_instructions_role").cloned().unwrap_or_default(),
        "project_context": prefix_metadata.get("project_context").cloned().unwrap_or_default(),
        "cwd": prefix_metadata.get("cwd").cloned().unwrap_or_default(),
    });
    let sink = Arc::new(PersistenceSink {
        store: store.clone(),
        session_id: session_id.clone(),
        prompt_snapshot,
        prompt_snapshot_written: Arc::new(Mutex::new(false)),
        prompt_context_evidence: Arc::new(prompt_context_evidence),
        started: invocation_started,
        tool_elapsed_ms: Arc::new(Mutex::new(BTreeMap::new())),
        current_turn_index: Arc::new(Mutex::new(None)),
        control: SmokeControl::None,
        control_handle: Some(control_handle.clone()),
        events: Some(Arc::clone(&events)),
        stream_events: stream_events.clone(),
        trace: trace.clone(),
        trace_warning_emitted,
        include_reasoning: options.include_reasoning,
        reasoning_effort: resolved.reasoning_effort.clone(),
        model_metadata: resolved.metadata.clone(),
        context_recorder: Some(context_recorder.clone()),
        prompt_display: options.prompt_display.clone(),
        selected_agent: selected_agent_summary.clone(),
        prompt_prefix_metadata: Some(prompt_prefix_metadata.clone()),
    });
    if let Some(object) = generation_metadata.as_object_mut() {
        object.insert("prompt_prefix".to_string(), prompt_prefix_metadata);
        let mut context_counting = context_counting_metadata(
            &prompt_assembly.prompt_instructions,
            turn_prompt_instructions.len(),
            previous_messages.len(),
            prompt_assembly.prefix_contextual_user_messages.len(),
            selected_skill_context_message_count,
            prompt_skills
                .iter()
                .map(|skill| skill.name.clone())
                .collect(),
        );
        if let Some(context_counting) = context_counting.as_object_mut() {
            context_counting.insert(
                "hook_context_message_count".to_string(),
                json!(hook_context_message_count),
            );
        }
        object.insert("context_counting".to_string(), context_counting);
    }
    let request = AgentLoopRequest {
        model_provider: resolved.provider.clone(),
        model: resolved.model.clone(),
        generation_metadata,
        prompt_instructions: prompt_assembly.prompt_instructions,
        turn_prompt_instructions,
        previous_messages,
        context_messages: mailbox_context_messages,
        prefix_contextual_user_messages: prompt_assembly.prefix_contextual_user_messages,
        turn_contextual_user_messages,
        prompt_messages: vec![
            prompt_message_from_inputs_with_options(
                &options.prompt,
                &options.image_inputs,
                &cwd,
                &resolved.metadata,
                options.extract_prompt_image_sources,
            )?
            .message,
        ],
        tools,
        tool_search: tool_search_options,
        max_turns: DEFAULT_AGENT_MAX_TURNS,
    };
    let completion = match run_agent_loop(Arc::clone(&provider), request, sink, control_receivers)
        .await
    {
        Ok(completion) => completion,
        Err(err) => {
            let err = Error::from(err);
            if !overflow_retry_attempted && !controlled_run && is_context_overflow_error(&err) {
                store.delete_messages_from_seq(&session_id, prompt_session_seq)?;
                compact_session(CompactSessionOptions {
                    state: options.state.clone(),
                    cwd: cwd.clone(),
                    session: session_id.clone(),
                    config_path: options.config_path.clone(),
                    model: options
                        .model
                        .clone()
                        .or_else(|| Some(format!("{}/{}", resolved.provider, resolved.model))),
                    reasoning_effort: options
                        .reasoning_effort
                        .clone()
                        .or_else(|| resolved.reasoning_effort.clone()),
                    inherited_env: Some(loaded.env.clone()),
                    reason: CompactionReason::Overflow,
                    instructions: Some(
                        "Prioritize preserving the current task state before retrying an overflowed request."
                            .to_string(),
                    ),
                    force: true,
                })
                .await?;
                let mut retry_options = options.clone();
                retry_options.session = Some(session_id.clone());
                retry_options.continue_latest = false;
                return Box::pin(run_live_internal(
                    retry_options,
                    source,
                    continue_sources,
                    stream_events_after.clone(),
                    None,
                    true,
                ))
                .await;
            }
            interrupt_exec_sessions_for_task(&session_id);
            return Err(err);
        }
    };
    if completion.outcome == Outcome::Aborted {
        interrupt_exec_sessions_for_task(&session_id);
    } else {
        detach_exec_sessions_for_task(session_id.clone());
    }
    record_missed_required_agents(
        &store,
        &session_id,
        &completion.messages,
        &required_agent_mentions,
    )?;
    let final_answer = completion
        .messages
        .iter()
        .rev()
        .find_map(assistant_text)
        .unwrap_or_default();
    if let Some(runtime) = &hook_runtime_for_lifecycle {
        let _ = runtime.run_post_llm_call(&json!({
            "session_id": session_id,
            "outcome": completion.outcome.as_str(),
            "assistant_text": final_answer,
        }));
        let stop = runtime.run_stop(&json!({
            "session_id": session_id,
            "outcome": completion.outcome.as_str(),
        }));
        if let Some(reason) = stop.block_reason {
            return Err(Error::Message(reason));
        }
    }
    let tool_failures = completion
        .messages
        .iter()
        .filter(|message| matches!(message, Message::ToolResult { is_error: true, .. }))
        .count();
    if should_title_completed_session(
        created_session,
        first_use_empty_visible_session,
        completion.outcome,
    ) {
        ensure_new_visible_session_title(
            &store,
            &session_id,
            &options.prompt,
            &selected_skills,
            &skill_catalog,
            provider_for_title,
            &title_resolved,
        )
        .await?;
    }

    tokio::task::yield_now().await;
    let mut events = events.lock().expect("event lock poisoned").clone();
    let context_snapshot = context_recorder.latest_snapshot();
    if let Some(snapshot) = &context_snapshot {
        let value = serde_json::to_value(snapshot)?;
        events.push(value.clone());
        if let Some(stream) = stream_events_after {
            stream(RunStreamEvent::value(value));
        }
    }
    let mut warnings = project_instructions.warnings;
    warnings.extend(extension_warnings);
    warnings.extend(mcp_warnings);
    warnings.append(&mut tool_surface_warnings);
    if let Some(runtime) = &hook_runtime_for_lifecycle {
        let _ = runtime.run_session_end(&json!({
            "session_id": session_id,
            "outcome": completion.outcome.as_str(),
        }));
    }
    Ok(RunResult {
        session_id,
        outcome: completion.outcome,
        terminal_reason: completion.terminal_reason,
        final_answer,
        db_path: options.state.db_path().to_path_buf(),
        cwd,
        provider: resolved.provider,
        model: resolved.model,
        base_url: resolved.base_url,
        api_key_env: resolved.api_key_env,
        reasoning_effort: resolved.reasoning_effort,
        context_limit: resolved.context_limit,
        tool_failures,
        selected_agent: selected_agent_summary,
        selected_skills,
        context_snapshot,
        events,
        warnings,
    })
}

fn hook_contextual_user_messages(
    context: &[Value],
) -> Vec<psychevo_agent_core::ContextualUserMessage> {
    let blocks = context
        .iter()
        .filter_map(hook_context_block)
        .collect::<Vec<_>>();
    if blocks.is_empty() {
        Vec::new()
    } else {
        vec![
            psychevo_agent_core::ContextualUserMessage::new_with_category(
                "hook_context",
                "turn_context",
                blocks,
            ),
        ]
    }
}

fn hook_context_block(value: &Value) -> Option<psychevo_agent_core::ContextualUserBlock> {
    if let Some(text) = value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        return Some(psychevo_agent_core::ContextualUserBlock::new(
            "hook_context",
            None,
            None,
            text.to_string(),
        ));
    }
    let object = value.as_object()?;
    let text = object
        .get("text")
        .or_else(|| object.get("message"))
        .or_else(|| object.get("content"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())?;
    let source_name = object
        .get("source")
        .or_else(|| object.get("source_name"))
        .or_else(|| object.get("sourceName"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let source_path = object
        .get("source_path")
        .or_else(|| object.get("sourcePath"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Some(psychevo_agent_core::ContextualUserBlock::new(
        "hook_context",
        source_name,
        source_path,
        text.to_string(),
    ))
}

include!("run_loop/helpers.rs");
