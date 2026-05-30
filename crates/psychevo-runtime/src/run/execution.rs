#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) async fn run_live_internal(
    options: RunOptions,
    source: &str,
    continue_sources: &[&str],
    stream_events: Option<RunStreamSink>,
    control: Option<RunControl>,
    overflow_retry_attempted: bool,
) -> Result<RunResult> {
    let workdir = canonical_workdir(&options.workdir)?;
    if options.prompt.trim().is_empty() && options.image_inputs.is_empty() {
        return Err(Error::Message("prompt is empty".to_string()));
    }

    if options.no_agents && options.agent.is_some() {
        return Err(Error::Config(
            "--agent cannot be used together with no_agents".to_string(),
        ));
    }
    let loaded = load_run_config(&options, &workdir)?;
    let project_context_mode = loaded.config.project_context.instructions;
    let project_instructions = load_project_instructions(&workdir, project_context_mode)?;
    let permission_mode = options.permission_mode.unwrap_or_default();
    let approval_mode = options.approval_mode.unwrap_or({
        match loaded.config.permissions.approvals_reviewer {
            crate::types::ApprovalsReviewer::User => crate::types::ApprovalMode::Manual,
            crate::types::ApprovalsReviewer::Smart => crate::types::ApprovalMode::Smart,
        }
    });
    let agents_home = resolve_agents_home(&loaded.env, &workdir)?;
    let agent_catalog = discover_agents(&AgentDiscoveryOptions {
        home: agents_home,
        workdir: workdir.clone(),
        env: loaded.env.clone(),
        explicit_inputs: options.agent.iter().cloned().collect(),
        no_agents: options.no_agents,
    })?;
    let selected_agent = match &options.agent {
        Some(input) => Some(resolve_agent_definition(
            &agent_catalog,
            input,
            &workdir,
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
    let skills_home = resolve_skills_home(&loaded.env, &workdir)?;
    let mut explicit_skill_inputs = options.skill_inputs.clone();
    if let Some(agent) = &selected_agent {
        explicit_skill_inputs.extend(agent.skills.clone());
    }
    let skill_options = SkillDiscoveryOptions {
        home: skills_home.clone(),
        workdir: workdir.clone(),
        config_path: options.config_path.clone(),
        env: loaded.env.clone(),
        explicit_inputs: explicit_skill_inputs.clone(),
        no_skills: options.no_skills,
    };
    let skill_catalog = discover_skills(&skill_options)?;
    let selected_skills = selected_skills_for_run(
        &skill_catalog,
        &options.prompt,
        &explicit_skill_inputs,
        &workdir,
        &loaded.env,
    );
    let selected_agent_summary = selected_agent_for_result(selected_agent.as_ref());
    let required_agent_catalog = if options.no_agents {
        Vec::new()
    } else {
        agent_catalog_for_selected_policy(&agent_catalog.agents, selected_agent.as_ref())
    };
    let required_agent_mentions = required_agent_mentions(&options.prompt, &required_agent_catalog);
    let skill_context_fragments = skill_context_fragments(&selected_skills, &skill_catalog)?;
    let selected_skill_context_message_count = skill_context_fragments.len();
    let store = options.state.store().clone();
    let (session_id, created_session) = if let Some(session_id) = options.session.clone() {
        store.resume_session(&session_id)?;
        (session_id, false)
    } else if options.continue_latest {
        if let Some(session_id) =
            store.latest_session_for_workdir_with_sources(&workdir, continue_sources)?
        {
            store.resume_session(&session_id)?;
            (session_id, false)
        } else {
            (
                store.create_session_with_metadata(
                    &workdir,
                    source,
                    &resolved.model,
                    &resolved.provider,
                    Some(json!({
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
                        "workdir": workdir.display().to_string(),
                        "selected_agent": selected_agent_summary.clone(),
                    })),
                )?,
                true,
            )
        }
    } else {
        (
            store.create_session_with_metadata(
                &workdir,
                source,
                &resolved.model,
                &resolved.provider,
                Some(json!({
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
                    "workdir": workdir.display().to_string(),
                    "selected_agent": selected_agent_summary.clone(),
                })),
            )?,
            true,
        )
    };

    store.cleanup_reverted_messages(&session_id)?;
    maybe_preflight_compact_session(
        &options,
        &workdir,
        &session_id,
        &resolved.provider,
        &resolved.model,
        &resolved.reasoning_effort,
        &loaded.env,
    )
    .await?;
    let prompt_snapshot = options.snapshot_root.as_ref().and_then(|root| {
        SnapshotStore::new(root.clone(), session_id.clone(), workdir.clone())
            .track()
            .ok()
            .flatten()
    });

    let run_start = json!({
        "type": "run_start",
        "source": source,
        "session_id": session_id.clone(),
        "provider": resolved.provider.clone(),
        "model": resolved.model.clone(),
        "db": options.state.db_path().to_path_buf(),
        "workdir": workdir.clone(),
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
        "selected_agent": selected_agent_summary.clone(),
        "agents_enabled": !options.no_agents,
        "agent_count": agent_catalog.agents.len(),
        "selected_skills": selected_skills.clone(),
    });
    if let Some(stream) = &stream_events {
        stream(RunStreamEvent::Event(run_start.clone()));
    }
    let events = Arc::new(Mutex::new(vec![run_start]));
    emit_warning_events(
        &project_instructions.warnings,
        &events,
        stream_events.as_ref(),
    );

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
    let context_recorder = ContextRecorder::default();
    let provider_for_title = Arc::clone(&provider);
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
            workdir: workdir.clone(),
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
            tool_selection: loaded.config.tools.clone(),
            custom_toolsets: loaded.config.toolsets.clone(),
            allowed_agent_names: selected_agent
                .as_ref()
                .and_then(|agent| agent.tool_policy.allowed_agents.clone()),
            denied_agent_names: selected_agent
                .as_ref()
                .map(|agent| agent.tool_policy.denied_agents.clone())
                .unwrap_or_default(),
            required_agent_names: required_agent_mentions.clone(),
            spawn_depth_remaining: None,
        })
    } else {
        None
    };
    let permission_runtime = PermissionRuntime::new(
        workdir.clone(),
        workdir.join(".psychevo"),
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
    let (mcp_tools, mcp_warnings) =
        crate::mcp::mcp_tool_bindings(&options.mcp_servers, &workdir, Some(&permission_runtime))
            .await;
    emit_warning_events(&mcp_warnings, &events, stream_events.as_ref());
    let mut tools = assemble_tool_surface(ToolSurfaceAssembly {
        workdir: workdir.clone(),
        task_id: session_id.clone(),
        mode: options.mode,
        lsp: loaded.config.lsp.clone(),
        allow_login_shell: loaded.config.permissions.allow_login_shell,
        stream_events: stream_events.clone(),
        env: loaded.env.clone(),
        path_prefixes: managed_tools.path_prefixes.clone(),
        tool_selection: loaded.config.tools.clone(),
        custom_toolsets: loaded.config.toolsets.clone(),
        clarify: if options.clarify_enabled {
            ClarifyToolSurface::enabled(clarify_control, stream_events.clone())
        } else {
            ClarifyToolSurface::Disabled
        },
        skills: (!options.no_skills || !explicit_skill_inputs.is_empty()).then_some(skill_options),
        extension_tools: mcp_tools,
        agents: agent_tools,
    });
    tools = apply_agent_tool_policy(tools, selected_agent.as_ref(), options.mode);
    tools = apply_agent_hooks(tools, selected_agent.as_ref(), &workdir);
    tools = permission_runtime.wrap_tools(tools);
    let effective_tool_names = effective_tool_names(&tools);
    let prompt_agents = if options.no_agents {
        Vec::new()
    } else {
        agent_catalog_for_prompt(&agent_catalog.agents, selected_agent.as_ref(), &tools)
    };
    let prompt_skills = if skill_catalog_visible_for_tools(&tools) {
        skill_catalog.skills.clone()
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
    let tool_declarations_hash = tool_declarations_hash(&tools);
    let prefix_metadata = json!({
        "mode": options.mode.as_str(),
        "permission_mode": permission_mode.as_str(),
        "approval_mode": approval_mode.as_str(),
        "project_context": {
            "instructions": project_context_mode.as_str(),
        },
        "workdir": workdir.display().to_string(),
        "selected_agent": selected_agent_summary.clone(),
        "agents_enabled": !options.no_agents,
        "effective_tools": effective_tool_names,
        "agent_catalog_visible": !prompt_agents.is_empty(),
        "visible_agents": prompt_agents.iter().map(|agent| agent.name.clone()).collect::<Vec<_>>(),
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
            workdir: &workdir,
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
    let turn_contextual_user_messages = skill_contextual_user_messages(&skill_context_fragments);
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
        "workdir": prefix_metadata.get("workdir").cloned().unwrap_or_default(),
    });
    let sink = Arc::new(PersistenceSink {
        store: store.clone(),
        session_id: session_id.clone(),
        prompt_snapshot,
        prompt_snapshot_written: Arc::new(Mutex::new(false)),
        prompt_context_evidence: Arc::new(prompt_context_evidence),
        started: Instant::now(),
        tool_elapsed_ms: Arc::new(Mutex::new(BTreeMap::new())),
        control: SmokeControl::None,
        control_handle: Some(control_handle.clone()),
        events: Some(Arc::clone(&events)),
        stream_events: stream_events.clone(),
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
        object.insert(
            "context_counting".to_string(),
            context_counting_metadata(
                &prompt_assembly.prompt_instructions,
                turn_prompt_instructions.len(),
                previous_messages.len(),
                prompt_assembly.prefix_contextual_user_messages.len(),
                selected_skill_context_message_count,
                prompt_skills
                    .iter()
                    .map(|skill| skill.name.clone())
                    .collect(),
            ),
        );
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
                &workdir,
                &resolved.metadata,
                options.extract_prompt_image_sources,
            )?
            .message,
        ],
        tools,
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
                    workdir: workdir.clone(),
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
    run_agent_hook_event(
        selected_agent.as_ref(),
        "Stop",
        &workdir,
        json!({ "outcome": completion.outcome.as_str() }),
    );
    let final_answer = completion
        .messages
        .iter()
        .rev()
        .find_map(assistant_text)
        .unwrap_or_default();
    let tool_failures = completion
        .messages
        .iter()
        .filter(|message| matches!(message, Message::ToolResult { is_error: true, .. }))
        .count();
    if created_session && source == "tui" && completion.outcome == Outcome::Normal {
        ensure_new_tui_session_title(
            &store,
            &session_id,
            &options.prompt,
            &selected_skills,
            &skill_catalog,
            provider_for_title,
            &resolved,
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
            stream(RunStreamEvent::Event(value));
        }
    }
    let mut warnings = project_instructions.warnings;
    warnings.extend(mcp_warnings);
    Ok(RunResult {
        session_id,
        outcome: completion.outcome,
        terminal_reason: completion.terminal_reason,
        final_answer,
        db_path: options.state.db_path().to_path_buf(),
        workdir,
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

pub(crate) fn selected_skills_for_run(
    catalog: &crate::skills::SkillCatalog,
    prompt: &str,
    explicit_inputs: &[String],
    workdir: &std::path::Path,
    env: &BTreeMap<String, String>,
) -> Vec<SelectedSkill> {
    let mut selected = select_explicit_skills(catalog, explicit_inputs, workdir, env);
    selected.extend(select_skills_for_prompt(catalog, prompt));
    let mut seen = std::collections::BTreeSet::new();
    selected
        .into_iter()
        .filter(|skill| seen.insert(skill.path.clone()))
        .collect()
}

pub(crate) async fn maybe_preflight_compact_session(
    options: &RunOptions,
    workdir: &std::path::Path,
    session_id: &str,
    provider: &str,
    model: &str,
    reasoning_effort: &Option<String>,
    env: &BTreeMap<String, String>,
) -> Result<()> {
    let model_override = options
        .model
        .clone()
        .or_else(|| Some(format!("{provider}/{model}")));
    let result = compact_session(CompactSessionOptions {
        state: options.state.clone(),
        workdir: workdir.to_path_buf(),
        session: session_id.to_string(),
        config_path: options.config_path.clone(),
        model: model_override,
        reasoning_effort: options
            .reasoning_effort
            .clone()
            .or_else(|| reasoning_effort.clone()),
        inherited_env: Some(env.clone()),
        reason: CompactionReason::AutoThreshold,
        instructions: None,
        force: false,
    })
    .await?;
    let _ = result;
    Ok(())
}

pub(crate) fn selected_agent_for_result(agent: Option<&AgentDefinition>) -> Option<SelectedAgent> {
    agent.map(|agent| SelectedAgent {
        name: agent.name.clone(),
        source: agent.source.as_str().to_string(),
        path: agent.file_path.clone(),
    })
}

pub(crate) fn session_model_metadata(metadata: &serde_json::Value) -> ModelMetadata {
    metadata
        .get("model_metadata")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

pub(crate) fn session_agent_input_from_metadata(metadata: &serde_json::Value) -> Option<String> {
    if let Some(main_agent) = metadata.get("main_agent") {
        if main_agent
            .get("mode")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|mode| mode == "default")
            || main_agent.is_null()
        {
            return None;
        }
        if let Some(input) = main_agent
            .get("input")
            .and_then(serde_json::Value::as_str)
            .or_else(|| main_agent.get("name").and_then(serde_json::Value::as_str))
            .or_else(|| main_agent.get("path").and_then(serde_json::Value::as_str))
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(input.to_string());
        }
    }
    metadata
        .get("selected_agent")
        .and_then(|value| {
            value
                .get("input")
                .or_else(|| value.get("name"))
                .or_else(|| value.get("path"))
        })
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) fn prompt_prefix_invalidation_reason(
    record: &PromptPrefixRecord,
    provider: &str,
    model: &str,
    mode: crate::types::RunMode,
    selected_agent: Option<&SelectedAgent>,
    tool_declarations_hash: &str,
    expected_metadata: &serde_json::Value,
) -> Option<String> {
    if record.provider != provider
        || record.model != model
        || record.tool_declarations_hash != tool_declarations_hash
    {
        return Some("runtime_context_changed".to_string());
    }
    let Some(metadata) = record.metadata.as_ref() else {
        return Some("prefix_metadata_missing".to_string());
    };
    if metadata.get("mode").and_then(serde_json::Value::as_str) != Some(mode.as_str()) {
        return Some("runtime_context_changed".to_string());
    }
    let expected_agent = serde_json::to_value(selected_agent).unwrap_or(serde_json::Value::Null);
    if metadata
        .get("selected_agent")
        .unwrap_or(&serde_json::Value::Null)
        != &expected_agent
    {
        return Some("main_agent_changed".to_string());
    }
    for key in [
        "effective_tools",
        "agent_catalog_visible",
        "visible_agents",
        "skill_catalog_visible",
        "project_instructions_visible",
        "project_instructions_role",
        "project_context",
        "workdir",
    ] {
        if metadata.get(key).unwrap_or(&serde_json::Value::Null)
            != expected_metadata
                .get(key)
                .unwrap_or(&serde_json::Value::Null)
        {
            return Some("runtime_context_changed".to_string());
        }
    }
    None
}

pub(crate) fn take_prompt_prefix_notice(
    store: &SqliteStore,
    session_id: &str,
) -> Result<Option<String>> {
    let notice = store
        .session_metadata(session_id)?
        .and_then(|metadata| metadata.get(PROMPT_PREFIX_NOTICE_METADATA_KEY).cloned())
        .and_then(|value| value.as_str().map(str::to_string));
    if notice.is_some() {
        store.set_session_metadata_field(session_id, PROMPT_PREFIX_NOTICE_METADATA_KEY, None)?;
    }
    Ok(notice)
}

pub(crate) fn required_agent_mentions(prompt: &str, agents: &[AgentDefinition]) -> Vec<String> {
    let known = agents
        .iter()
        .map(|agent| agent.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut found = std::collections::BTreeSet::new();
    for raw in prompt.split_whitespace() {
        let Some(rest) = raw.strip_prefix('@') else {
            continue;
        };
        let name = rest.trim_matches(|ch: char| {
            !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        });
        if known.contains(name) {
            found.insert(name.to_string());
        }
    }
    found.into_iter().collect()
}

pub(crate) fn smart_approval_handler(
    provider: Arc<dyn GenerationProvider>,
    resolved: &ResolvedRunProvider,
    config: &PermissionConfig,
    metadata: Value,
) -> Option<Arc<dyn ApprovalHandler>> {
    if config.approvals_reviewer != crate::types::ApprovalsReviewer::Smart {
        return None;
    }
    let model = config
        .auto_review
        .model
        .as_deref()
        .and_then(parse_provider_model)
        .unwrap_or_else(|| ModelTarget {
            provider: resolved.provider.clone(),
            model: resolved.model.clone(),
        });
    Some(Arc::new(SmartReviewerApprovalHandler {
        provider,
        model,
        metadata,
        timeout_secs: config.auto_review.timeout_secs,
    }))
}

pub(crate) fn parse_provider_model(value: &str) -> Option<ModelTarget> {
    let (provider, model) = value.trim().split_once('/')?;
    let provider = provider.trim();
    let model = model.trim();
    (!provider.is_empty() && !model.is_empty()).then(|| ModelTarget {
        provider: provider.to_string(),
        model: model.to_string(),
    })
}

#[derive(Clone)]
pub(crate) struct SmartReviewerApprovalHandler {
    pub(crate) provider: Arc<dyn GenerationProvider>,
    pub(crate) model: ModelTarget,
    pub(crate) metadata: Value,
    pub(crate) timeout_secs: u64,
}

impl std::fmt::Debug for SmartReviewerApprovalHandler {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SmartReviewerApprovalHandler")
            .field(
                "model",
                &format!("{}/{}", self.model.provider, self.model.model),
            )
            .finish_non_exhaustive()
    }
}

impl ApprovalHandler for SmartReviewerApprovalHandler {
    fn timeout_secs(&self) -> u64 {
        self.timeout_secs
    }

    fn request_permission(
        &self,
        request: PermissionApprovalRequest,
    ) -> futures::future::BoxFuture<'static, PermissionApprovalDecision> {
        let provider = Arc::clone(&self.provider);
        let model = self.model.clone();
        let metadata = self.metadata.clone();
        Box::pin(async move {
            smart_review_permission(provider, model, metadata, request)
                .await
                .unwrap_or_else(|_| PermissionApprovalDecision::deny())
        })
    }
}

pub(crate) async fn smart_review_permission(
    provider: Arc<dyn GenerationProvider>,
    model: ModelTarget,
    metadata: Value,
    request: PermissionApprovalRequest,
) -> Result<PermissionApprovalDecision> {
    let prompt = json!({
        "instruction": "Review this tool permission request. Return strict JSON only with decision allow or deny, risk, and rationale.",
        "request": {
            "tool": request.tool_name,
            "summary": request.summary,
            "reason": request.reason,
            "matched_rule": request.matched_rule,
            "suggested_rule": request.suggested_rule,
        }
    });
    let generation = GenerationRequest {
        model,
        messages: vec![json!({
            "role": "user",
            "content": prompt.to_string(),
        })],
        tools: Vec::new(),
        metadata,
    };
    let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
    let mut stream = provider
        .stream(generation, AbortSignal::new(abort_rx))
        .await
        .map_err(|err| Error::Message(err.to_string()))?;
    let mut text = String::new();
    while let Some(event) = stream.next().await {
        match event.map_err(|err| Error::Message(err.to_string()))? {
            StreamEvent::TextDelta { text: delta } => text.push_str(&delta),
            StreamEvent::Done { .. } => break,
            _ => {}
        }
    }
    let value: Value =
        serde_json::from_str(text.trim()).map_err(|err| Error::Message(err.to_string()))?;
    match value.get("decision").and_then(Value::as_str) {
        Some("allow") => Ok(PermissionApprovalDecision::allow_once()),
        Some("deny") => Ok(PermissionApprovalDecision::deny()),
        _ => Err(Error::Message(
            "smart reviewer JSON must include decision allow or deny".to_string(),
        )),
    }
}

pub(crate) fn record_missed_required_agents(
    store: &SqliteStore,
    session_id: &str,
    messages: &[Message],
    required: &[String],
) -> Result<()> {
    if required.is_empty() {
        return Ok(());
    }
    let called = called_agent_names(messages, required);
    let missed = required
        .iter()
        .filter(|name| !called.contains(*name))
        .cloned()
        .collect::<Vec<_>>();
    if missed.is_empty() {
        return Ok(());
    }
    let text = format!(
        "Required agent delegation was not performed: {}",
        missed.join(", ")
    );
    store.append_message_with_metrics(
        session_id,
        &user_text_message(text),
        None,
        Some(json!({
            "agent_notification": {
                "type": "missing_required_agent_call",
                "agents": missed,
                "hidden": true
            }
        })),
    )
}

#[cfg(test)]
mod smart_reviewer_tests {
    use super::*;
    use psychevo_ai::{FakeProvider, RawStreamEvent};

    fn request() -> PermissionApprovalRequest {
        PermissionApprovalRequest {
            tool_call_id: "call-1".to_string(),
            tool_name: "read".to_string(),
            summary: "/etc/hosts".to_string(),
            reason: "outside workdir".to_string(),
            matched_rule: None,
            suggested_rule: Some("filesystem:/etc/hosts".to_string()),
            allow_always: true,
            timeout_secs: 90,
        }
    }

    #[tokio::test]
    async fn smart_reviewer_allows_once_from_json() {
        let provider: Arc<dyn GenerationProvider> = Arc::new(FakeProvider::new(vec![vec![
            RawStreamEvent::Text(
                r#"{"decision":"allow","risk":"low","rationale":"read-only"}"#.to_string(),
            ),
            RawStreamEvent::Done(Outcome::Normal),
        ]]));
        let decision = smart_review_permission(
            provider,
            ModelTarget {
                provider: "mock".to_string(),
                model: "reviewer".to_string(),
            },
            json!({}),
            request(),
        )
        .await
        .expect("review");
        assert_eq!(
            decision.outcome,
            crate::types::PermissionApprovalOutcome::AllowOnce
        );
    }

    #[tokio::test]
    async fn smart_reviewer_fails_closed_on_malformed_json() {
        let provider: Arc<dyn GenerationProvider> = Arc::new(FakeProvider::new(vec![vec![
            RawStreamEvent::Text("not json".to_string()),
            RawStreamEvent::Done(Outcome::Normal),
        ]]));
        let err = smart_review_permission(
            provider,
            ModelTarget {
                provider: "mock".to_string(),
                model: "reviewer".to_string(),
            },
            json!({}),
            request(),
        )
        .await
        .expect_err("malformed JSON should fail");
        assert!(err.to_string().contains("expected ident"));
    }
}
