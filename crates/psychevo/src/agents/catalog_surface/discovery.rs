impl AgentRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PendingInit => "pending_init",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Errored => "errored",
            Self::Interrupted => "interrupted",
            Self::Shutdown => "shutdown",
            Self::NotFound => "not_found",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunRecord {
    pub id: String,
    pub task_name: Option<String>,
    pub agent_name: String,
    pub task: String,
    pub parent_session_id: String,
    pub child_session_id: Option<String>,
    pub role: AgentInvocationRole,
    pub background: bool,
    pub status: AgentRunStatus,
    pub edge_status: Option<AgentEdgeStatus>,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub outcome: Option<String>,
    pub final_answer: Option<String>,
    pub error: Option<String>,
    #[serde(default)]
    pub effective_max_spawn_depth: Option<u8>,
    #[serde(default)]
    pub team_run_id: Option<String>,
    #[serde(default)]
    pub mission_run_id: Option<String>,
    #[serde(default)]
    pub team_name: Option<String>,
    #[serde(default)]
    pub team_member_id: Option<String>,
    #[serde(default)]
    pub agent_path: Option<String>,
}

pub type AgentRun = AgentRunRecord;

#[derive(Clone)]
pub struct AgentControl {
    pub(crate) supervisor: AgentSupervisor,
    pub(crate) store: Option<StateRuntime>,
}

impl std::fmt::Debug for AgentControl {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentControl")
            .finish_non_exhaustive()
    }
}

impl AgentControl {
    pub(crate) fn new(supervisor: AgentSupervisor, store: Option<StateRuntime>) -> Self {
        Self { supervisor, store }
    }

    #[doc(hidden)]
    pub fn __standalone(store: Option<StateRuntime>) -> Self {
        Self::new(AgentSupervisor::default(), store)
    }

    pub async fn status_value(&self) -> Value {
        self.status_value_for(None, false).await
    }

    pub async fn status_value_for(
        &self,
        parent_session_id: Option<&str>,
        all: bool,
    ) -> Value {
        agent_status_value(
            &self.supervisor,
            self.store.as_ref(),
            parent_session_id,
            all,
        )
        .await
    }

    pub async fn status_records(
        &self,
        parent_session_id: Option<&str>,
        all: bool,
    ) -> Vec<AgentRunRecord> {
        agent_status_records(
            &self.supervisor,
            self.store.as_ref(),
            parent_session_id,
            all,
        )
        .await
    }

    pub async fn wait(
        &self,
        id: &str,
        timeout: Duration,
    ) -> Result<Option<AgentRunRecord>> {
        wait_agent_id(&self.supervisor, id, self.store.as_ref(), timeout).await
    }

    pub async fn close(&self, id: &str) -> Result<Option<AgentRunRecord>> {
        close_agent_id(&self.supervisor, id, self.store.as_ref()).await
    }

    pub async fn send(&self, id: &str, message: &str) -> Result<Option<AgentRunRecord>> {
        send_agent_message(&self.supervisor, id, message, self.store.as_ref()).await
    }

    pub async fn stop_with_grace(
        &self,
        id: &str,
        grace: Duration,
    ) -> Result<Option<AgentRunRecord>> {
        stop_agent_id_with_grace(&self.supervisor, id, self.store.as_ref(), grace).await
    }

    pub async fn resume(&self, id: &str) -> Result<Option<AgentRunRecord>> {
        resume_agent_id(&self.supervisor, id, self.store.as_ref()).await
    }

    pub fn spawning_paused(&self, parent_session_id: &str) -> bool {
        self.supervisor.spawning_paused(parent_session_id)
    }

    pub fn set_spawning_paused(&self, parent_session_id: &str, paused: bool) -> bool {
        self.supervisor
            .set_spawning_paused(parent_session_id, paused)
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawAgentFrontmatter {
    pub(crate) name: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) tools: Option<Value>,
    #[serde(rename = "disallowedTools")]
    pub(crate) disallowed_tools: Option<Value>,
    pub(crate) permission: Option<Value>,
    pub(crate) permissions: Option<Value>,
    #[serde(rename = "permissionMode")]
    pub(crate) permission_mode: Option<Value>,
    #[serde(rename = "mcpServers")]
    pub(crate) mcp_servers: Option<Value>,
    pub(crate) skills: Option<Value>,
    #[serde(rename = "optionalContributions", alias = "optional_contributions")]
    pub(crate) optional_contributions: Option<Value>,
    pub(crate) hooks: Option<Value>,
    pub(crate) background: Option<bool>,
    #[serde(rename = "initialPrompt")]
    pub(crate) initial_prompt: Option<String>,
    #[serde(rename = "maxTurns")]
    pub(crate) max_turns: Option<usize>,
    #[serde(rename = "maxSpawnDepth", alias = "max_spawn_depth")]
    pub(crate) max_spawn_depth: Option<u8>,
    #[serde(rename = "projectInstructions", alias = "project_instructions")]
    pub(crate) project_instructions: Option<Value>,
    pub(crate) effort: Option<String>,
    pub(crate) backend: Option<Value>,
    pub(crate) entrypoints: Option<Value>,
    pub(crate) kind: Option<Value>,
    pub(crate) enabled: Option<Value>,
    pub(crate) label: Option<Value>,
    pub(crate) command: Option<Value>,
    pub(crate) args: Option<Value>,
    pub(crate) env: Option<Value>,
    #[serde(rename = "clientCapabilities", alias = "client_capabilities")]
    pub(crate) client_capabilities: Option<Value>,
    pub(crate) cwd: Option<Value>,
    pub(crate) memory: Option<Value>,
    pub(crate) isolation: Option<Value>,
}

#[derive(Clone)]
pub(crate) struct AgentToolContext {
    pub(crate) provider: Provider,
    pub(crate) model_provider: String,
    pub(crate) model: String,
    pub(crate) provider_label: String,
    pub(crate) base_url: String,
    pub(crate) api_key_env: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) context_limit: Option<u64>,
    pub(crate) generation_metadata: Value,
    pub(crate) cwd: PathBuf,
    pub(crate) mode: RunMode,
    pub(crate) project_context_mode: ProjectContextInstructionMode,
    pub(crate) permission_config: PermissionConfig,
    pub(crate) lsp: LspConfig,
    pub(crate) permission_mode: PermissionMode,
    pub(crate) approval_handler: Option<Arc<dyn ApprovalHandler>>,
    pub(crate) state: StateRuntime,
    pub(crate) config_path: Option<PathBuf>,
    pub(crate) protected_config_paths: Vec<PathBuf>,
    pub(crate) parent_session_id: String,
    pub(crate) parent_context_snapshot: Vec<Message>,
    pub(crate) catalog: AgentCatalog,
    pub(crate) control_handle: Option<ControlHandle>,
    pub(crate) stream_events: Option<RunStreamSink>,
    pub(crate) workspace_mutations: Option<crate::types::WorkspaceMutationSink>,
    pub(crate) model_metadata: ModelMetadata,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) path_prefixes: Vec<PathBuf>,
    pub(crate) sandbox_policy: crate::sandbox::SandboxPolicy,
    pub(crate) home: PathBuf,
    pub(crate) image_input_enabled: bool,
    pub(crate) image_generation: Option<crate::config::ResolvedImageGenerationConfig>,
    pub(crate) web_search: crate::config::WebSearchConfig,
    pub(crate) tool_selection: ToolSelectionConfig,
    pub(crate) custom_toolsets: BTreeMap<String, CustomToolsetConfig>,
    pub(crate) extension_inputs: crate::extensions::AcceptedExtensionInputs,
    pub(crate) allowed_agent_names: Option<BTreeSet<String>>,
    pub(crate) denied_agent_names: BTreeSet<String>,
    pub(crate) required_agent_names: Vec<String>,
    pub(crate) spawn_depth_remaining: Option<u8>,
    pub(crate) active_team: Option<ActiveAgentTeamContext>,
    pub(crate) external_delegate: Option<Arc<dyn crate::types::ExternalAgentDelegate>>,
    pub(crate) supervisor: AgentSupervisor,
}

pub fn discover_agents(options: &AgentDiscoveryOptions) -> Result<AgentCatalog> {
    if options.no_agents {
        return Ok(AgentCatalog::default());
    }

    let mut catalog = AgentCatalog::default();
    let mut winners: BTreeMap<String, PathBuf> = BTreeMap::new();

    for input in &options.explicit_inputs {
        if input.trim().is_empty() {
            continue;
        }
        if let Some(path) = existing_agent_path(input, &options.cwd, &options.env)? {
            load_agent_file(&mut catalog, &mut winners, &path, AgentSource::Explicit)?;
        }
    }

    load_agent_dir(
        &mut catalog,
        &mut winners,
        &options.cwd.join(".psychevo").join("agents"),
        AgentSource::Project,
    )?;

    for (dir, source) in ancestor_compatible_agent_dirs(&options.cwd) {
        load_agent_dir(&mut catalog, &mut winners, &dir, source)?;
    }

    load_agent_dir(
        &mut catalog,
        &mut winners,
        &options.home.join("agents"),
        AgentSource::Global,
    )?;

    if let Ok(home) = home_path(&options.env) {
        load_agent_dir(
            &mut catalog,
            &mut winners,
            &home.join(".agents").join("agents"),
            AgentSource::AgentsGlobal,
        )?;
        load_agent_dir(
            &mut catalog,
            &mut winners,
            &home.join(".claude").join("agents"),
            AgentSource::ClaudeGlobal,
        )?;
    }

    match crate::config::load_agent_backend_configs(&options.home, &options.cwd, &options.env) {
        Ok(backends) => {
            for backend in backends.values() {
                if let Some(agent) = generated_agent_from_backend(backend) {
                    insert_agent(&mut catalog, &mut winners, agent);
                }
            }
        }
        Err(err) => catalog.diagnostics.push(AgentDiagnostic::warning(
            format!("failed to load agent backends: {err}"),
            None,
        )),
    }

    for agent in built_in_agents() {
        insert_agent(&mut catalog, &mut winners, agent);
    }

    Ok(catalog)
}

pub fn resolve_agent_definition(
    catalog: &AgentCatalog,
    input: &str,
    cwd: &Path,
    env: &BTreeMap<String, String>,
) -> Result<AgentDefinition> {
    if let Some(path) = existing_agent_path(input, cwd, env)? {
        let agent = parse_agent_file(&path, AgentSource::Explicit)?;
        if !agent.enabled {
            return Err(Error::Config(format!("agent `{}` is disabled", agent.name)));
        }
        return Ok(agent);
    }

    catalog
        .agents
        .iter()
        .find(|agent| agent.name == input)
        .cloned()
        .ok_or_else(|| Error::Config(format!("unknown agent: {input}")))
}

pub(crate) fn generated_agent_from_backend(
    backend: &AgentBackendConfig,
) -> Option<AgentDefinition> {
    if !backend.enabled {
        return None;
    }
    let description = backend
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            let label = backend.label.trim();
            (!label.is_empty()).then_some(label)
        })
        .unwrap_or(&backend.id)
        .to_string();
    let mut diagnostics = Vec::new();
    if backend.command.is_none() {
        diagnostics.push(AgentDiagnostic::warning(
            format!("agent backend `{}` is missing command", backend.id),
            None,
        ));
    }
    Some(AgentDefinition {
        name: backend.id.clone(),
        description,
        instructions: String::new(),
        enabled: true,
        file_path: None,
        source: AgentSource::Generated,
        backend: Some(AgentBackendRef {
            name: backend.id.clone(),
        }),
        entrypoints: backend.entrypoints.clone(),
        model: None,
        tool_policy: AgentToolPolicy {
            mcp_servers: backend.mcp_servers.clone(),
            ..AgentToolPolicy::default()
        },
        skills: Vec::new(),
        optional_contributions: BTreeSet::new(),
        hooks: None,
        background: None,
        initial_prompt: None,
        max_turns: None,
        max_spawn_depth: 0,
        project_instructions: None,
        effort: None,
        diagnostics,
    })
}

pub fn list_agents_value(catalog: &AgentCatalog) -> Value {
    json!({
        "agents": catalog.agents.iter().map(|agent| {
            json!({
                "name": agent.name,
                "description": agent.description,
                "enabled": agent.enabled,
                "source": agent.source.as_str(),
                "source_label": agent.source.display_label(),
                "generated": agent.source == AgentSource::Generated,
                "path": agent.file_path,
                "backend": agent.backend,
                "entrypoints": agent.entrypoints,
                "model": agent.model,
                "tools": agent.tool_policy.allowed,
                "disallowed_tools": agent.tool_policy.denied,
                "allowed_agents": agent.tool_policy.allowed_agents,
                "disallowed_agents": agent.tool_policy.denied_agents,
                "permission_mode": agent.tool_policy.permission_mode,
                "max_spawn_depth": agent.max_spawn_depth,
                "project_instructions": agent.project_instructions,
                "effective_policy": agent_effective_policy_value(agent, Some(catalog)),
                "diagnostics": agent.diagnostics,
            })
        }).collect::<Vec<_>>(),
        "shadowed_agents": catalog.shadowed_agents.iter().map(|agent| {
            json!({
                "name": agent.name,
                "description": agent.description,
                "enabled": agent.enabled,
                "source": agent.source.as_str(),
                "source_label": agent.source.display_label(),
                "generated": agent.source == AgentSource::Generated,
                "path": agent.file_path,
                "backend": agent.backend,
                "entrypoints": agent.entrypoints,
                "model": agent.model,
                "tools": agent.tool_policy.allowed,
                "disallowed_tools": agent.tool_policy.denied,
                "allowed_agents": agent.tool_policy.allowed_agents,
                "disallowed_agents": agent.tool_policy.denied_agents,
                "permission_mode": agent.tool_policy.permission_mode,
                "max_spawn_depth": agent.max_spawn_depth,
                "project_instructions": agent.project_instructions,
                "effective_policy": agent_effective_policy_value(agent, Some(catalog)),
                "diagnostics": agent.diagnostics,
            })
        }).collect::<Vec<_>>(),
        "disabled_agents": catalog.disabled_agents.iter().map(|agent| {
            json!({
                "name": agent.name,
                "description": agent.description,
                "enabled": agent.enabled,
                "source": agent.source.as_str(),
                "source_label": agent.source.display_label(),
                "generated": agent.source == AgentSource::Generated,
                "path": agent.file_path,
                "backend": agent.backend,
                "entrypoints": agent.entrypoints,
                "model": agent.model,
                "tools": agent.tool_policy.allowed,
                "disallowed_tools": agent.tool_policy.denied,
                "allowed_agents": agent.tool_policy.allowed_agents,
                "disallowed_agents": agent.tool_policy.denied_agents,
                "permission_mode": agent.tool_policy.permission_mode,
                "max_spawn_depth": agent.max_spawn_depth,
                "project_instructions": agent.project_instructions,
                "effective_policy": agent_effective_policy_value(agent, Some(catalog)),
                "diagnostics": agent.diagnostics,
            })
        }).collect::<Vec<_>>(),
        "diagnostics": catalog.diagnostics,
    })
}

pub fn view_agent_value(agent: &AgentDefinition) -> Value {
    view_agent_value_with_catalog(agent, None)
}

pub fn view_agent_value_with_catalog(
    agent: &AgentDefinition,
    catalog: Option<&AgentCatalog>,
) -> Value {
    json!({
        "name": agent.name,
        "description": agent.description,
        "instructions": agent.instructions,
        "enabled": agent.enabled,
        "source": agent.source.as_str(),
        "source_label": agent.source.display_label(),
        "generated": agent.source == AgentSource::Generated,
        "path": agent.file_path,
        "backend": agent.backend,
        "entrypoints": agent.entrypoints,
        "model": agent.model,
        "tools": agent.tool_policy.allowed,
        "disallowed_tools": agent.tool_policy.denied,
        "allowed_agents": agent.tool_policy.allowed_agents,
        "disallowed_agents": agent.tool_policy.denied_agents,
        "permissions": agent.tool_policy.permissions,
        "permission_mode": agent.tool_policy.permission_mode,
        "mcp_servers": agent.tool_policy.mcp_servers,
        "skills": agent.skills,
        "optional_contributions": agent.optional_contributions,
        "hooks": agent.hooks,
        "background": agent.background,
        "initial_prompt": agent.initial_prompt,
        "max_turns": agent.max_turns,
        "max_spawn_depth": agent.max_spawn_depth,
        "project_instructions": agent.project_instructions,
        "effort": agent.effort,
        "tool_policy": {
            "tools": agent.tool_policy.allowed,
            "disallowed_tools": agent.tool_policy.denied,
            "allowed_agents": agent.tool_policy.allowed_agents,
            "disallowed_agents": agent.tool_policy.denied_agents,
            "permissions": agent.tool_policy.permissions,
            "permission_mode": agent.tool_policy.permission_mode,
            "mcp_servers": agent.tool_policy.mcp_servers,
        },
        "effective_policy": agent_effective_policy_value(agent, catalog),
        "diagnostics": agent.diagnostics,
    })
}

pub fn agent_effective_policy_value(
    agent: &AgentDefinition,
    catalog: Option<&AgentCatalog>,
) -> Value {
    let tools_mode = match &agent.tool_policy.allowed {
        None => "inherit",
        Some(allowed) if allowed.is_empty() => "explicit_empty",
        Some(_) => "explicit_allowlist",
    };
    let agent_catalog_visible = agent_policy_allows_agent_catalog(agent);
    let visible_agents = catalog.filter(|_| agent_catalog_visible).map(|catalog| {
        agent_catalog_for_policy(agent, &catalog.agents)
            .into_iter()
            .map(|agent| agent.name)
            .collect::<Vec<_>>()
    });
    json!({
        "tools": {
            "mode": tools_mode,
            "allowed": agent.tool_policy.allowed,
            "denied": agent.tool_policy.denied,
        },
        "agent_catalog": {
            "visible": agent_catalog_visible,
            "agents": visible_agents,
        },
        "skill_catalog": {
            "visible": agent_policy_allows_skill_catalog(agent),
        },
        "project_instructions": {
            "visible": agent_project_instructions_enabled(Some(agent)),
            "raw": agent.project_instructions,
        },
    })
}

pub(crate) fn agent_project_instructions_enabled(agent: Option<&AgentDefinition>) -> bool {
    agent.is_none_or(|agent| agent.project_instructions != Some(false))
}

pub fn format_agents_for_prompt(catalog: &[AgentDefinition]) -> String {
    if catalog.is_empty() {
        return String::new();
    }
    let mut text = String::from(prompt_templates::agent_catalog_intro());
    text.push_str("\n<agents>");
    for agent in catalog {
        text.push_str("\n<agent name=\"");
        text.push_str(&agent.name);
        text.push_str("\" source=\"");
        text.push_str(agent.source.as_str());
        text.push_str("\">");
        text.push_str(&agent.description);
        text.push_str("</agent>");
    }
    text.push_str("\n</agents>");
    text
}

pub(crate) fn format_selected_agent_instruction(
    agent: &AgentDefinition,
    role: AgentInvocationRole,
) -> String {
    match role {
        AgentInvocationRole::Main => prompt_templates::selected_main_agent(
            &agent.name,
            &agent.description,
            &agent.instructions,
        ),
        AgentInvocationRole::Subagent | AgentInvocationRole::Fork => {
            prompt_templates::selected_child_agent(
                &agent.name,
                &agent.description,
                &agent.instructions,
            )
        }
        AgentInvocationRole::System => prompt_templates::selected_system_agent(
            &agent.name,
            &agent.description,
            &agent.instructions,
        ),
    }
}

pub(crate) fn apply_agent_tool_policy(
    tools: Vec<Arc<dyn ToolBinding>>,
    agent: Option<&AgentDefinition>,
    mode: RunMode,
) -> Vec<Arc<dyn ToolBinding>> {
    tools
        .into_iter()
        .filter(|tool| agent_allows_tool(tool.name(), agent, mode))
        .collect()
}

pub(crate) fn narrow_permission_mode_for_agent(
    parent: PermissionMode,
    agent: Option<&AgentDefinition>,
) -> PermissionMode {
    let Some(agent_mode) = agent.and_then(|agent| agent.tool_policy.permission_mode) else {
        return parent;
    };
    match agent_mode {
        AgentPermissionMode::Plan => parent,
        AgentPermissionMode::Default => match parent {
            PermissionMode::AcceptEdits | PermissionMode::BypassPermissions => {
                PermissionMode::Default
            }
            PermissionMode::Default | PermissionMode::DontAsk => parent,
        },
        AgentPermissionMode::AcceptEdits => match parent {
            PermissionMode::AcceptEdits | PermissionMode::BypassPermissions => {
                PermissionMode::AcceptEdits
            }
            PermissionMode::Default | PermissionMode::DontAsk => parent,
        },
    }
}

pub(crate) fn effective_run_mode(
    parent: RunMode,
    agent: Option<&AgentDefinition>,
) -> RunMode {
    if parent == RunMode::Plan
        || agent.is_some_and(|agent| {
            agent.tool_policy.permission_mode == Some(AgentPermissionMode::Plan)
        })
    {
        RunMode::Plan
    } else {
        parent
    }
}

pub(crate) fn effective_tool_names(tools: &[Arc<dyn ToolBinding>]) -> Vec<String> {
    tools.iter().map(|tool| tool.name().to_string()).collect()
}

pub(crate) fn agent_catalog_for_prompt(
    catalog: &[AgentDefinition],
    selected_agent: Option<&AgentDefinition>,
    tools: &[Arc<dyn ToolBinding>],
) -> Vec<AgentDefinition> {
    if !tools.iter().any(|tool| tool.name() == "spawn_agent") {
        return Vec::new();
    }
    agent_catalog_for_selected_policy(catalog, selected_agent)
}

pub(crate) fn agent_catalog_for_selected_policy(
    catalog: &[AgentDefinition],
    selected_agent: Option<&AgentDefinition>,
) -> Vec<AgentDefinition> {
    match selected_agent {
        Some(agent) => agent_catalog_for_policy(agent, catalog),
        None => catalog
            .iter()
            .filter(|agent| agent.supports_entrypoint(AgentEntrypoint::Subagent))
            .cloned()
            .collect(),
    }
}

pub(crate) fn skill_catalog_visible_for_tools(tools: &[Arc<dyn ToolBinding>]) -> bool {
    let has_list = tools.iter().any(|tool| tool.name() == "list_skills");
    let has_view = tools.iter().any(|tool| tool.name() == "view_skill");
    has_list && has_view
}

pub(crate) fn agent_policy_allows_agent_spawn(agent: &AgentDefinition) -> bool {
    agent_policy_allows_agent_catalog(agent)
}

pub(crate) fn build_hook_runtime(
    agent: Option<&AgentDefinition>,
    plugin_hook_sources: Vec<crate::hooks::HookSourceDescriptor>,
    mut config: crate::hooks::HookRuntimeConfig,
    cwd: &Path,
) -> Option<crate::hooks::HookRuntime> {
    if let Some(agent) = agent.filter(|agent| agent.hooks.is_some())
        && let Some(source) = crate::hooks::agent_hook_source(agent)
    {
        config.sources.push(source);
    }
    config.sources.extend(plugin_hook_sources);
    if config.sources.is_empty() {
        return None;
    };
    Some(crate::hooks::HookRuntime::new(
        cwd.to_path_buf(),
        config,
    ))
}

pub(crate) fn apply_hook_runtime(
    tools: Vec<Arc<dyn ToolBinding>>,
    hook_runtime: crate::hooks::HookRuntime,
) -> Vec<Arc<dyn ToolBinding>> {
    tools
        .into_iter()
        .map(|tool| {
            Arc::new(HookedTool {
                inner: tool,
                hook_runtime: hook_runtime.clone(),
            }) as Arc<dyn ToolBinding>
        })
        .collect()
}

pub(crate) fn apply_runtime_hooks(
    tools: Vec<Arc<dyn ToolBinding>>,
    agent: Option<&AgentDefinition>,
    plugin_hook_sources: Vec<crate::hooks::HookSourceDescriptor>,
    config: crate::hooks::HookRuntimeConfig,
    cwd: &Path,
) -> Vec<Arc<dyn ToolBinding>> {
    match build_hook_runtime(agent, plugin_hook_sources, config, cwd) {
        Some(runtime) => apply_hook_runtime(tools, runtime),
        None => tools,
    }
}

pub(crate) fn agent_tools(context: AgentToolContext) -> Vec<Arc<dyn ToolBinding>> {
    let mut tools = Vec::<Arc<dyn ToolBinding>>::new();
    if context.spawn_depth_remaining != Some(0) {
        tools.push(Arc::new(SpawnAgentTool::new(context.clone())));
    }
    tools.push(Arc::new(ListAgentsTool::new(context.clone())));
    tools.push(Arc::new(WaitAgentTool::new(context.clone())));
    tools.push(Arc::new(SendMessageTool::new(context.clone())));
    tools.push(Arc::new(CloseAgentTool::new(context.clone())));
    tools.push(Arc::new(ResumeAgentTool::new(context)));
    tools
}

pub(crate) async fn agent_status_value(
    supervisor: &AgentSupervisor,
    store: Option<&StateRuntime>,
    parent_session_id: Option<&str>,
    all: bool,
) -> Value {
    let records = agent_status_records(supervisor, store, parent_session_id, all).await;
    let spawning_paused = parent_session_id
        .is_some_and(|parent| supervisor.spawning_paused(parent));
    json!({
        "agents": records,
        "control": {
            "spawning_paused": spawning_paused,
            "max_spawn_depth_cap": MAX_AGENT_SPAWN_DEPTH_CAP,
            "concurrency_cap": MAX_TEAM_PARALLEL_AGENTS_CAP,
        }
    })
}

pub(crate) async fn agent_status_model_value(
    supervisor: &AgentSupervisor,
    store: Option<&StateRuntime>,
    parent_session_id: Option<&str>,
    all: bool,
) -> Value {
    let records = agent_status_records(supervisor, store, parent_session_id, all).await;
    let mut agents = Vec::with_capacity(records.len());
    for record in &records {
        agents.push(subagent_summary_value(store, record, true).await);
    }
    json!({
        "agents": agents,
        "control": {
            "spawning_paused": parent_session_id
                .is_some_and(|parent| supervisor.spawning_paused(parent)),
            "max_spawn_depth_cap": MAX_AGENT_SPAWN_DEPTH_CAP,
        }
    })
}

pub(crate) async fn agent_status_records(
    supervisor: &AgentSupervisor,
    store: Option<&StateRuntime>,
    parent_session_id: Option<&str>,
    all: bool,
) -> Vec<AgentRunRecord> {
    let mut records = Vec::new();
    let mut scope_sessions = BTreeSet::new();
    if let Some(store) = store {
        let edges = if all {
            store.list_agent_edges().await.unwrap_or_default()
        } else if let Some(parent) = parent_session_id {
            scope_sessions.insert(parent.to_string());
            collect_agent_edge_tree(store, parent)
                .await
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        for edge in &edges {
            scope_sessions.insert(edge.child_session_id.clone());
        }
        for edge in edges {
            records.push(agent_record_from_edge(store, edge).await);
        }
    }
    let runs = supervisor.slots();
    for state in runs.values() {
        if !all && let Some(parent) = parent_session_id {
            let in_scope = state.record.parent_session_id == parent
                || scope_sessions.contains(&state.record.parent_session_id);
            if !in_scope {
                continue;
            }
        }
        if !records.iter().any(|record| record.id == state.record.id) {
            records.push(state.record.clone());
        }
    }
    records.sort_by_key(|record| std::cmp::Reverse(record.started_at_ms));
    records
}

pub(crate) async fn wait_agent_id(
    supervisor: &AgentSupervisor,
    id: &str,
    store: Option<&StateRuntime>,
    timeout: Duration,
) -> Result<Option<AgentRunRecord>> {
    let started = Instant::now();
    loop {
        if let Some(record) = resolve_supervised_record(supervisor, id)?
            && agent_status_is_final(record.status)
        {
            return Ok(Some(record));
        }
        if let Some(store) = store
            && let Some(edge) = find_agent_edge_for_target(store, id).await?
        {
            let record = agent_record_from_edge(store, edge).await;
            if agent_status_is_final(record.status) {
                return Ok(Some(record));
            }
        }
        if started.elapsed() >= timeout {
            if let Some(record) = resolve_supervised_record(supervisor, id)? {
                return Ok(Some(record));
            }
            return if let Some(store) = store
                && let Some(edge) = find_agent_edge_for_target(store, id).await?
            {
                Ok(Some(agent_record_from_edge(store, edge).await))
            } else {
                Ok(None)
            };
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn resolve_supervised_record(
    supervisor: &AgentSupervisor,
    id: &str,
) -> Result<Option<AgentRunRecord>> {
    let runs = supervisor.slots();
    resolve_live_record_locked(&runs, id)
}

pub async fn wait_agent_mailbox(
    parent_session_id: &str,
    timeout: Duration,
    store: &StateRuntime,
) -> Result<Value> {
    let started = Instant::now();
    loop {
        if store
            .has_pending_agent_mailbox_events(parent_session_id)
            .await?
        {
            return Ok(json!({
                "message": "Wait completed.",
                "timed_out": false,
            }));
        }
        if started.elapsed() >= timeout {
            return Ok(json!({
                "message": "Wait timed out.",
                "timed_out": true,
            }));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

pub(crate) async fn close_agent_id(
    supervisor: &AgentSupervisor,
    id: &str,
    store: Option<&StateRuntime>,
) -> Result<Option<AgentRunRecord>> {
    let mut pending_terminal = None;
    let live = {
        let mut runs = supervisor.slots();
        if let Some((live_id, previous)) = resolve_live_key_and_record_locked(&runs, id)? {
            if runs
                .get(&live_id)
                .is_some_and(|state| state.phase == AgentRunPhase::PendingTerminal)
            {
                pending_terminal = Some((live_id, previous));
                None
            } else {
                let child_session = {
                    let state = runs.get_mut(&live_id).expect("live record exists");
                    if let Some(control) = &state.control {
                        control.stop();
                    }
                    state.record.status = AgentRunStatus::Shutdown;
                    state.record.edge_status = Some(AgentEdgeStatus::Closed);
                    state.record.ended_at_ms = Some(now_ms());
                    state.record.outcome = Some("shutdown".to_string());
                    state.record.child_session_id.clone()
                };
                if let Some(child_session) = child_session.as_deref() {
                    close_live_descendants_locked(&mut runs, child_session);
                }
                Some((live_id, previous, child_session))
            }
        } else {
            None
        }
    };
    if let Some((live_id, previous)) = pending_terminal {
        if let Some(store) = store {
            supervisor.retry_terminal(store, &live_id).await?;
        }
        return Ok(Some(previous));
    }
    let Some((_live_id, previous, _child_session)) = live else {
        return close_persisted_agent(id, store).await;
    };
    Ok(Some(previous))
}

async fn close_persisted_agent(
    id: &str,
    store: Option<&StateRuntime>,
) -> Result<Option<AgentRunRecord>> {
    if let Some(store) = store
        && let Some(edge) = find_agent_edge_for_target(store, id).await?
    {
        let previous = agent_record_from_edge(store, edge.clone()).await;
        store.close_agent_edge_subtree(&edge.child_session_id).await?;
        return Ok(Some(previous));
    }
    Ok(None)
}

pub(crate) async fn stop_agent_id_with_grace(
    supervisor: &AgentSupervisor,
    id: &str,
    store: Option<&StateRuntime>,
    grace: Duration,
) -> Result<Option<AgentRunRecord>> {
    let requested = request_agent_stop_id(supervisor, id)?;
    if requested.is_none() {
        return close_agent_id(supervisor, id, store).await;
    }
    tokio::time::sleep(grace).await;
    force_stop_agent_id(supervisor, id, store).await
}

pub(crate) fn request_agent_stop_id(
    supervisor: &AgentSupervisor,
    id: &str,
) -> Result<Option<AgentRunRecord>> {
    let runs = supervisor.slots();
    let Some((live_id, record)) = resolve_live_key_and_record_locked(&runs, id)? else {
        return Ok(None);
    };
    if agent_status_is_final(record.status) {
        return Ok(Some(record));
    }
    if let Some(state) = runs.get(&live_id)
        && let Some(control) = &state.control
    {
        control.stop();
    }
    Ok(Some(record))
}
