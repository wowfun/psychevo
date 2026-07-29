use super::*;

impl fmt::Debug for AdapterTurnOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdapterTurnOptions")
            .field("snapshot_root", &self.snapshot_root)
            .field("max_context_messages", &self.max_context_messages)
            .field(
                "selected_capability_root_count",
                &self.selected_capability_roots.len(),
            )
            .field(
                "has_workspace_mutations",
                &self.workspace_mutations.is_some(),
            )
            .field("input_part_count", &self.input_parts.len())
            .field(
                "has_run_stream_observer",
                &self.run_stream_observer.is_some(),
            )
            .field(
                "initial_thread_preference_count",
                &self.initial_thread_preferences.len(),
            )
            .field(
                "has_prepared_source_key",
                &self.prepared_source_key.is_some(),
            )
            .field(
                "has_turn_event_observer",
                &self.turn_event_observer.is_some(),
            )
            .field("agent_entrypoint", &self.agent_entrypoint)
            .finish()
    }
}

impl fmt::Debug for PreparedTurnControl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PreparedTurnControl(..)")
    }
}

impl TurnRequest {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            image_inputs: Vec::new(),
            extract_prompt_image_sources: true,
            prompt_display: None,
            client_turn_id: None,
            source: "sdk".to_string(),
            config_path: None,
            model: None,
            reasoning_effort: None,
            runtime_ref: None,
            runtime_options: BTreeMap::new(),
            include_reasoning: false,
            mode: RunMode::default(),
            permission_mode: None,
            approval_handler: None,
            clarify_enabled: false,
            inherited_env: None,
            project_context: None,
            sandbox: None,
            agent: None,
            no_agents: false,
            no_skills: false,
            skill_inputs: Vec::new(),
            mcp_servers: Vec::new(),
            tools: Vec::new(),
            adapter_options: AdapterTurnOptions::default(),
            requested_turn_id: None,
            prepared_control: None,
        }
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    pub fn image_inputs(&self) -> &[ImageInput] {
        &self.image_inputs
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn clarify_enabled(&self) -> bool {
        self.clarify_enabled
    }

    pub fn with_prompt_images(
        mut self,
        image_inputs: Vec<ImageInput>,
        extract_prompt_image_sources: bool,
    ) -> Self {
        self.image_inputs = image_inputs;
        self.extract_prompt_image_sources = extract_prompt_image_sources;
        self
    }

    pub fn with_prompt_display(
        mut self,
        prompt_display: Option<crate::types::PromptDisplayMetadata>,
    ) -> Self {
        self.prompt_display = prompt_display;
        self
    }

    pub fn with_identity(
        mut self,
        source: impl Into<String>,
        client_turn_id: Option<String>,
    ) -> Self {
        self.source = source.into();
        self.client_turn_id = client_turn_id;
        self
    }

    pub fn with_model(mut self, model: Option<String>, reasoning_effort: Option<String>) -> Self {
        self.model = model;
        self.reasoning_effort = reasoning_effort;
        self
    }

    pub fn with_runtime(
        mut self,
        runtime_ref: Option<String>,
        runtime_options: BTreeMap<String, String>,
    ) -> Self {
        self.runtime_ref = runtime_ref;
        self.runtime_options = runtime_options;
        self
    }

    pub fn with_reasoning_output(mut self, include_reasoning: bool) -> Self {
        self.include_reasoning = include_reasoning;
        self
    }

    pub fn with_execution_policy(
        mut self,
        mode: RunMode,
        permission_mode: Option<PermissionMode>,
        config_path: Option<PathBuf>,
    ) -> Self {
        self.mode = mode;
        self.permission_mode = permission_mode;
        self.config_path = config_path;
        self
    }

    pub fn with_approval(
        mut self,
        approval_handler: Option<Arc<dyn ApprovalHandler>>,
        clarify_enabled: bool,
    ) -> Self {
        self.approval_handler = approval_handler;
        self.clarify_enabled = clarify_enabled;
        self
    }

    pub fn with_environment(
        mut self,
        inherited_env: Option<BTreeMap<String, String>>,
        project_context: Option<ProjectContextInstructionMode>,
        sandbox: Option<RunSandboxOverride>,
    ) -> Self {
        self.inherited_env = inherited_env;
        self.project_context = project_context;
        self.sandbox = sandbox;
        self
    }

    pub fn with_agent(mut self, agent: Option<String>, no_agents: bool, no_skills: bool) -> Self {
        self.agent = agent;
        self.no_agents = no_agents;
        self.no_skills = no_skills;
        self
    }

    pub fn with_skills(mut self, skill_inputs: Vec<String>) -> Self {
        self.skill_inputs = skill_inputs;
        self
    }

    pub fn with_mcp_servers(mut self, mcp_servers: Vec<McpServerInput>) -> Self {
        self.mcp_servers = mcp_servers;
        self
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __set_runtime_tools(&mut self, tools: Vec<RuntimeTool>) {
        self.tools = tools;
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __from_run_options(
        options: RunOptions,
        source: impl Into<String>,
        run_stream_observer: Option<RunStreamSink>,
    ) -> Self {
        Self {
            prompt: options.prompt,
            image_inputs: options.image_inputs,
            extract_prompt_image_sources: options.extract_prompt_image_sources,
            prompt_display: options.prompt_display,
            client_turn_id: None,
            source: source.into(),
            config_path: options.config_path,
            model: options.model,
            reasoning_effort: options.reasoning_effort,
            runtime_ref: options.runtime_ref,
            runtime_options: options.runtime_options,
            include_reasoning: options.include_reasoning,
            mode: options.mode,
            permission_mode: options.permission_mode,
            approval_handler: options.approval_handler,
            clarify_enabled: options.clarify_enabled,
            inherited_env: options.inherited_env,
            project_context: options.project_context_override,
            sandbox: options.sandbox_override,
            agent: options.agent,
            no_agents: options.no_agents,
            no_skills: options.no_skills,
            skill_inputs: options.skill_inputs,
            mcp_servers: options.mcp_servers,
            tools: options.runtime_tools,
            adapter_options: AdapterTurnOptions {
                snapshot_root: options.snapshot_root,
                max_context_messages: options.max_context_messages,
                selected_capability_roots: options.selected_capability_roots,
                workspace_mutations: options.workspace_mutations,
                run_stream_observer,
                ..AdapterTurnOptions::default()
            },
            requested_turn_id: None,
            prepared_control: None,
        }
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __set_control(
        &mut self,
        handle: crate::types::RunControlHandle,
        control: crate::types::RunControl,
    ) {
        self.prepared_control = Some(PreparedTurnControl { handle, control });
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __set_adapter_options(&mut self, options: AdapterTurnOptions) {
        self.adapter_options = options;
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __take_adapter_input_parts(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.adapter_options.input_parts)
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __take_run_stream_observer(&mut self) -> Option<RunStreamSink> {
        self.adapter_options.run_stream_observer.take()
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __take_initial_thread_preferences(&mut self) -> BTreeMap<String, String> {
        std::mem::take(&mut self.adapter_options.initial_thread_preferences)
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __take_prepared_source_key(&mut self) -> Option<String> {
        self.adapter_options.prepared_source_key.take()
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __take_agent_entrypoint(&mut self) -> Option<crate::agents::AgentEntrypoint> {
        self.adapter_options.agent_entrypoint.take()
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __set_agent_entrypoint(&mut self, entrypoint: crate::agents::AgentEntrypoint) {
        self.adapter_options.agent_entrypoint = Some(entrypoint);
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __set_turn_id(&mut self, turn_id: String) {
        self.requested_turn_id = Some(turn_id);
    }

    pub fn tool(mut self, tool: Arc<dyn psychevo_agent_core::ToolBinding>) -> Self {
        self.tools.push(RuntimeTool::new(tool));
        self
    }

    pub(super) fn into_run_options(
        self,
        state: StateRuntime,
        cwd: PathBuf,
        thread_id: String,
        application_config_path: Option<PathBuf>,
    ) -> RunOptions {
        RunOptions {
            state,
            cwd,
            snapshot_root: self.adapter_options.snapshot_root,
            session: Some(thread_id),
            continue_latest: false,
            prompt: self.prompt,
            image_inputs: self.image_inputs,
            extract_prompt_image_sources: self.extract_prompt_image_sources,
            prompt_display: self.prompt_display,
            max_context_messages: self.adapter_options.max_context_messages,
            config_path: self.config_path.or(application_config_path),
            project_context_override: self.project_context,
            sandbox_override: self.sandbox,
            model: self.model,
            reasoning_effort: self.reasoning_effort,
            runtime_ref: self.runtime_ref,
            runtime_session_id: None,
            runtime_options: self.runtime_options,
            include_reasoning: self.include_reasoning,
            mode: self.mode,
            permission_mode: self.permission_mode,
            approval_handler: self.approval_handler,
            clarify_enabled: self.clarify_enabled,
            inherited_env: self.inherited_env,
            agent: self.agent,
            external_agent_delegate: None,
            no_agents: self.no_agents,
            no_skills: self.no_skills,
            selected_capability_roots: self.adapter_options.selected_capability_roots,
            skill_inputs: self.skill_inputs,
            mcp_servers: self.mcp_servers,
            mcp_runtime: self.adapter_options.mcp_runtime,
            workspace_mutations: self.adapter_options.workspace_mutations,
            runtime_tools: self.tools,
        }
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __into_run_options(
        self,
        state: StateRuntime,
        cwd: PathBuf,
        thread_id: String,
        application_config_path: Option<PathBuf>,
    ) -> RunOptions {
        self.into_run_options(state, cwd, thread_id, application_config_path)
    }
}
