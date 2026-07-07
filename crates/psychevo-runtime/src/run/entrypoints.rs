#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) const TITLE_GENERATION_TIMEOUT_SECS: u64 = 15;
pub(crate) const DEFAULT_AGENT_MAX_TURNS: usize = 128;
pub(crate) const SESSION_TITLE_MAX_CHARS: usize = 100;

pub async fn run_live(options: RunOptions) -> Result<RunResult> {
    run_live_internal(options, "run", &["run"], None, None, false).await
}

pub async fn run_live_streaming(
    options: RunOptions,
    source: &str,
    continue_sources: &[&str],
    stream: RunStreamSink,
) -> Result<RunResult> {
    run_live_internal(options, source, continue_sources, Some(stream), None, false).await
}

pub async fn run_live_streaming_controlled(
    options: RunOptions,
    source: &str,
    continue_sources: &[&str],
    stream: RunStreamSink,
    control: RunControl,
) -> Result<RunResult> {
    run_live_internal(
        options,
        source,
        continue_sources,
        Some(stream),
        Some(control),
        false,
    )
    .await
}

pub fn reload_session_context(options: ReloadContextOptions) -> Result<ReloadContextResult> {
    let store = options.state.store().clone();
    let summary = store
        .session_summary(&options.session)?
        .ok_or_else(|| Error::Message(format!("session not found: {}", options.session)))?;
    let metadata = store.session_metadata(&summary.id)?.unwrap_or(json!({}));
    let cwd = canonical_cwd(std::path::Path::new(&summary.cwd))?;
    let mode = options
        .mode
        .or_else(|| {
            metadata
                .get("mode")
                .and_then(serde_json::Value::as_str)
                .and_then(crate::types::RunMode::parse)
        })
        .unwrap_or_default();
    let env = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| std::env::vars().collect());
    let project_context_options = RunOptions {
        state: options.state.clone(),
        cwd: cwd.clone(),
        snapshot_root: None,
        session: Some(summary.id.clone()),
        continue_latest: false,
        prompt: String::new(),
        image_inputs: Vec::new(),
        extract_prompt_image_sources: true,
        prompt_display: None,
        max_context_messages: None,
        config_path: options.config_path.clone(),
        project_context_override: None,
        sandbox_override: None,
        model: Some(format!("{}/{}", summary.provider, summary.model)),
        reasoning_effort: metadata
            .get("reasoning_effort")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        runtime_ref: None,
        runtime_session_id: None,
        runtime_options: std::collections::BTreeMap::new(),
        include_reasoning: false,
        mode,
        permission_mode: None,
        approval_mode: None,
        approval_handler: None,
        clarify_enabled: false,
        inherited_env: Some(env.clone()),
        agent: None,
        external_agent_delegate: None,
        no_agents: options.no_agents,
        no_skills: options.no_skills,
        selected_capability_roots: Vec::new(),
        skill_inputs: Vec::new(),
        mcp_servers: Vec::new(),
        runtime_tools: Vec::new(),
    };
    let (plugin_policy, plugin_env, home) =
        load_plugin_policy_config_lenient(&project_context_options, &cwd)?;
    let extension_assembly =
        crate::extensions::assemble_extensions(crate::extensions::ExtensionAssemblyInput {
            home: &home,
            cwd: &cwd,
            env: &plugin_env,
            plugin_policy: &plugin_policy,
            selected_capability_roots: &[],
            mcp_servers: Vec::new(),
            runtime_tools: Vec::new(),
        });
    let project_context_mode =
        load_project_context_instruction_mode(&project_context_options, &cwd)?;
    let agents_home = resolve_agents_home(&env, &cwd)?;
    let agent_input =
        main_agent_input_from_sources(options.no_agents, options.agent.as_deref(), Some(&metadata));
    let mut agent_explicit_inputs = agent_input.iter().cloned().collect::<Vec<_>>();
    agent_explicit_inputs.extend(extension_assembly.agent_inputs.clone());
    let agent_catalog = discover_agents(&AgentDiscoveryOptions {
        home: agents_home,
        cwd: cwd.clone(),
        env: env.clone(),
        explicit_inputs: agent_explicit_inputs,
        no_agents: options.no_agents,
    })?;
    let selected_agent = match &agent_input {
        Some(input) if !options.no_agents => {
            Some(resolve_agent_definition(&agent_catalog, input, &cwd, &env)?)
        }
        _ => None,
    };
    let skills_home = resolve_skills_home(&env, &cwd)?;
    let mut explicit_skill_inputs = Vec::new();
    if let Some(agent) = &selected_agent {
        explicit_skill_inputs.extend(agent.skills.clone());
    }
    let skill_options = SkillDiscoveryOptions {
        home: skills_home,
        cwd: cwd.clone(),
        config_path: options.config_path.clone(),
        env: env.clone(),
        explicit_inputs: explicit_skill_inputs,
        additional_roots: extension_assembly.skill_inputs.clone(),
        no_skills: options.no_skills,
    };
    let skill_catalog = discover_skills(&skill_options)?;
    let project_instructions = load_project_instructions(&cwd, project_context_mode)?;
    let model_metadata = session_model_metadata(&metadata);
    let agent_tools = if !options.no_agents {
        let provider: Arc<dyn GenerationProvider> = Arc::new(OpenAiChatProvider::new(
            String::new(),
            String::new(),
            summary.provider.clone(),
        ));
        Some(AgentToolContext {
            provider,
            model_provider: summary.provider.clone(),
            model: summary.model.clone(),
            provider_label: metadata
                .get("provider_label")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(summary.provider.as_str())
                .to_string(),
            base_url: metadata
                .get("base_url")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            api_key_env: metadata
                .get("api_key_env")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            reasoning_effort: metadata
                .get("reasoning_effort")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            context_limit: metadata
                .get("context_limit")
                .and_then(serde_json::Value::as_u64),
            generation_metadata: json!({
                "model_metadata": model_metadata.public_json(),
            }),
            cwd: cwd.clone(),
            mode,
            project_context_mode,
            permission_config: PermissionConfig::default(),
            lsp: Default::default(),
            permission_mode: Default::default(),
            approval_mode: Default::default(),
            approval_handler: None,
            state: options.state.clone(),
            config_path: options.config_path.clone(),
            parent_session_id: summary.id.clone(),
            parent_context_snapshot: Vec::new(),
            catalog: agent_catalog.clone(),
            control_handle: None,
            stream_events: None,
            model_metadata: model_metadata.clone(),
            env: env.clone(),
            path_prefixes: Vec::new(),
            sandbox_policy: crate::sandbox::SandboxPolicy::disabled(),
            home: home.clone(),
            image_input_enabled:
                !crate::prompt_image::model_metadata_explicitly_disallows_image_input(
                    &model_metadata,
                ),
            image_generation: None,
            tool_selection: Default::default(),
            custom_toolsets: BTreeMap::new(),
            extension_inputs: extension_assembly.accepted_inputs(),
            allowed_agent_names: selected_agent
                .as_ref()
                .and_then(|agent| agent.tool_policy.allowed_agents.clone()),
            denied_agent_names: selected_agent
                .as_ref()
                .map(|agent| agent.tool_policy.denied_agents.clone())
                .unwrap_or_default(),
            required_agent_names: Vec::new(),
            spawn_depth_remaining: None,
            external_delegate: None,
        })
    } else {
        None
    };
    let extension_tools = extension_assembly.registry.runtime_tools();
    let tool_surface = assemble_tool_surface_with_warnings(ToolSurfaceAssembly {
        cwd: cwd.clone(),
        task_id: summary.id.clone(),
        mode,
        lsp: Default::default(),
        allow_login_shell: false,
        stream_events: None,
        env: BTreeMap::new(),
        path_prefixes: Vec::new(),
        sandbox_policy: crate::sandbox::SandboxPolicy::disabled(),
        sandbox_grants: crate::sandbox::SandboxWriteGrants::default(),
        home: Some(home.clone()),
        image_input_enabled: !crate::prompt_image::model_metadata_explicitly_disallows_image_input(
            &model_metadata,
        ),
        image_generation: None,
        tool_selection: Default::default(),
        custom_toolsets: BTreeMap::new(),
        contributed_toolsets: extension_assembly.toolsets.clone(),
        clarify: ClarifyToolSurface::Disabled,
        skills: (!options.no_skills).then_some(skill_options),
        extension_tools,
        agents: agent_tools,
    });
    let mut tools = tool_surface.tools;
    tools = apply_agent_tool_policy(tools, selected_agent.as_ref(), mode);
    let hook_config = crate::hooks::HookRuntimeConfig::default();
    tools = apply_runtime_hooks(
        tools,
        selected_agent.as_ref(),
        extension_assembly.hook_sources.clone(),
        hook_config,
        &cwd,
    );
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
        .then(|| developer_provider_role(&model_metadata.capabilities).to_string());
    let tool_declarations_hash = tool_declarations_hash(&tools);
    let selected_agent_summary = selected_agent_for_result(selected_agent.as_ref());
    let assembly = assemble_main_prompt_prefix(MainPromptPrefixInput {
        mode,
        cwd: &cwd,
        selected_agent: selected_agent.as_ref(),
        agents: &prompt_agents,
        skills: &prompt_skills,
        project_instruction_fragments: prompt_project_instructions,
        capabilities: &model_metadata.capabilities,
        tools_available: !tools.is_empty(),
    });
    let record = prompt_prefix_record(PromptPrefixRecordInput {
        session_id: &summary.id,
        provider: &summary.provider,
        model: &summary.model,
        prefix_hash: assembly.prefix_hash,
        tool_declarations_hash,
        invalidation_reason: Some(options.invalidation_reason),
        slots: assembly.prefix_slots,
        metadata: Some(json!({
            "mode": mode.as_str(),
            "selected_agent": selected_agent_summary,
            "agents_enabled": !options.no_agents,
            "effective_tools": effective_tool_names,
            "accepted_toolsets": tool_surface.accepted_toolset_names,
            "agent_catalog_visible": !prompt_agents.is_empty(),
            "visible_agents": prompt_agents.iter().map(|agent| agent.name.clone()).collect::<Vec<_>>(),
            "skill_catalog_visible": !prompt_skills.is_empty(),
            "project_instructions_visible": !prompt_project_instructions.is_empty(),
            "project_instructions_role": project_instructions_role,
            "project_context": {
                "instructions": project_context_mode.as_str(),
            },
            "cwd": cwd.display().to_string(),
        })),
    });
    let record = store.upsert_session_prompt_prefix(record)?;
    if let Some(notice) = options.notice {
        store.set_session_metadata_field(
            &summary.id,
            PROMPT_PREFIX_NOTICE_METADATA_KEY,
            Some(serde_json::Value::String(notice)),
        )?;
    }
    Ok(ReloadContextResult {
        session_id: summary.id,
        prefix_hash: record.prefix_hash,
        version: record.version,
        provider: record.provider,
        model: record.model,
        invalidation_reason: record.invalidation_reason,
    })
}

pub async fn spawn_agent_background(options: AgentSpawnOptions) -> Result<AgentSpawnResult> {
    let cwd = canonical_cwd(&options.cwd)?;
    if options.prompt.trim().is_empty() {
        return Err(Error::Message("agent message is empty".to_string()));
    }
    let run_options = RunOptions {
        state: options.state.clone(),
        cwd: cwd.clone(),
        snapshot_root: None,
        session: options.parent_session.clone(),
        continue_latest: false,
        prompt: options.prompt.clone(),
        image_inputs: Vec::new(),
        extract_prompt_image_sources: true,
        prompt_display: None,
        max_context_messages: None,
        config_path: options.config_path.clone(),
        project_context_override: None,
        sandbox_override: None,
        model: options.model.clone(),
        reasoning_effort: options.reasoning_effort.clone(),
        runtime_ref: None,
        runtime_session_id: None,
        runtime_options: std::collections::BTreeMap::new(),
        include_reasoning: false,
        mode: options.mode,
        permission_mode: options.permission_mode,
        approval_mode: options.approval_mode,
        approval_handler: options.approval_handler.clone(),
        clarify_enabled: false,
        inherited_env: options.inherited_env.clone(),
        agent: options.selected_parent_agent.clone(),
        external_agent_delegate: None,
        no_agents: false,
        no_skills: options.no_skills,
        selected_capability_roots: options.selected_capability_roots.clone(),
        skill_inputs: options.skill_inputs.clone(),
        mcp_servers: options.mcp_servers.clone(),
        runtime_tools: Vec::new(),
    };
    let loaded = load_run_config(&run_options, &cwd)?;
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
            runtime_tools: Vec::new(),
        });
    let permission_mode = options.permission_mode.unwrap_or_default();
    let approval_mode = options.approval_mode.unwrap_or({
        match loaded.config.permissions.approvals_reviewer {
            crate::types::ApprovalsReviewer::User => crate::types::ApprovalMode::Manual,
            crate::types::ApprovalsReviewer::Smart => crate::types::ApprovalMode::Smart,
        }
    });
    let agents_home = resolve_agents_home(&loaded.env, &cwd)?;
    let mut explicit_agent_inputs = options
        .selected_parent_agent
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    explicit_agent_inputs.extend(extension_assembly.agent_inputs.clone());
    let agent_catalog = discover_agents(&AgentDiscoveryOptions {
        home: agents_home,
        cwd: cwd.clone(),
        env: loaded.env.clone(),
        explicit_inputs: explicit_agent_inputs,
        no_agents: false,
    })?;
    let selected_parent_agent = match &options.selected_parent_agent {
        Some(input) => Some(resolve_agent_definition(
            &agent_catalog,
            input,
            &cwd,
            &loaded.env,
        )?),
        None => None,
    };
    let permission_mode =
        narrow_permission_mode_for_agent(permission_mode, selected_parent_agent.as_ref());
    let child_agent = resolve_agent_definition(&agent_catalog, &options.agent, &cwd, &loaded.env)?;
    if selected_parent_agent
        .as_ref()
        .is_some_and(|agent| !agent_policy_allows_agent_spawn(agent))
    {
        return Err(Error::Config(
            "agent spawning is not allowed by selected-agent tool policy".to_string(),
        ));
    }
    if let Some(allowed) = selected_parent_agent
        .as_ref()
        .and_then(|agent| agent.tool_policy.allowed_agents.as_ref())
        && !allowed.contains(&child_agent.name)
    {
        return Err(Error::Config(format!(
            "agent `{}` is not allowed by selected-agent tool policy",
            child_agent.name
        )));
    }
    if selected_parent_agent
        .as_ref()
        .is_some_and(|agent| agent.tool_policy.denied_agents.contains(&child_agent.name))
    {
        return Err(Error::Config(format!(
            "agent `{}` is denied by selected-agent tool policy",
            child_agent.name
        )));
    }
    let mut resolved_options = run_options.clone();
    if resolved_options.model.is_none()
        && let Some(model) = selected_parent_agent
            .as_ref()
            .and_then(|agent| agent.model.clone())
    {
        resolved_options.model = Some(model);
    }
    if resolved_options.reasoning_effort.is_none()
        && let Some(effort) = selected_parent_agent
            .as_ref()
            .and_then(|agent| agent.effort.clone())
    {
        resolved_options.reasoning_effort = Some(effort);
    }
    let resolved = resolve_run_provider(&resolved_options, &loaded)?;
    let managed_tools = ensure_rg(&loaded.env).await?;
    let store = options.state.store().clone();
    let selected_parent_summary = selected_agent_for_result(selected_parent_agent.as_ref());
    let parent_session_id = if let Some(session_id) = options.parent_session.clone() {
        store.resume_session(&session_id)?;
        session_id
    } else {
        store.create_session_with_metadata(
            &cwd,
            "tui",
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
                    "instructions": loaded.config.project_context.instructions.as_str(),
                },
                "cwd": cwd.display().to_string(),
                "selected_agent": selected_parent_summary,
            })),
        )?
    };
    let image_generation =
        crate::config::resolve_image_generation_config_from_loaded(&loaded, None, None, None, None)
            .ok();
    let image_input_enabled =
        !crate::prompt_image::model_metadata_explicitly_disallows_image_input(&resolved.metadata);
    let provider: Arc<dyn GenerationProvider> = Arc::new(OpenAiChatProvider::new(
        resolved.base_url.clone(),
        resolved.api_key.clone(),
        resolved.provider.clone(),
    ));
    let context = AgentToolContext {
        provider,
        model_provider: resolved.provider.clone(),
        model: resolved.model.clone(),
        provider_label: resolved.display_label.clone(),
        base_url: resolved.base_url.clone(),
        api_key_env: resolved.api_key_env.clone(),
        reasoning_effort: resolved.reasoning_effort.clone(),
        context_limit: resolved.context_limit,
        generation_metadata: json!({
            "model_metadata": resolved.metadata.public_json(),
            "reasoning_effort": resolved.reasoning_effort.clone(),
        }),
        cwd: cwd.clone(),
        mode: options.mode,
        project_context_mode: loaded.config.project_context.instructions,
        permission_config: loaded.config.permissions.clone(),
        lsp: loaded.config.lsp.clone(),
        permission_mode,
        approval_mode,
        approval_handler: options.approval_handler.clone(),
        state: options.state.clone(),
        config_path: options.config_path.clone(),
        parent_session_id: parent_session_id.clone(),
        parent_context_snapshot: Vec::new(),
        catalog: agent_catalog,
        control_handle: None,
        stream_events: None,
        model_metadata: resolved.metadata,
        env: loaded.env.clone(),
        path_prefixes: managed_tools.path_prefixes.clone(),
        sandbox_policy: crate::sandbox::SandboxPolicy::from_config(
            &loaded.config.sandbox,
            &cwd,
            options.mode,
            &loaded.env,
        )?,
        home,
        image_input_enabled,
        image_generation,
        tool_selection: loaded.config.tools.clone(),
        custom_toolsets: loaded.config.toolsets.clone(),
        extension_inputs: extension_assembly.accepted_inputs(),
        allowed_agent_names: selected_parent_agent
            .as_ref()
            .and_then(|agent| agent.tool_policy.allowed_agents.clone()),
        denied_agent_names: selected_parent_agent
            .as_ref()
            .map(|agent| agent.tool_policy.denied_agents.clone())
            .unwrap_or_default(),
        required_agent_names: Vec::new(),
        spawn_depth_remaining: None,
        external_delegate: None,
    };
    let agent = spawn_child_agent_background(context, child_agent, options.prompt)?;
    Ok(AgentSpawnResult {
        parent_session_id,
        agent,
    })
}
