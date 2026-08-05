use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use super::{
    AgentEnvironmentOverlay, AgentExecutionPolicy, AgentInputPart, AgentMissionRegistration,
    AgentModelSelection, AgentPreparationToken, AgentTargetSelection, AgentTurnInput,
    ResolvedCapabilityPlan, ResolvedTurnPlan, TurnAdmissionCancellation, TurnRequest,
};
use crate::types::{
    ApprovalHandler, ImageInput, McpServerInput, PermissionMode, ProjectContextInstructionMode,
    RunMode, RunSandboxOverride, RuntimeTool,
};

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
            include_reasoning: false,
            mode: RunMode::default(),
            permission_mode: None,
            approval_handler: None,
            clarify_enabled: false,
            inherited_env: None,
            project_context: None,
            sandbox: None,
            no_agents: false,
            no_skills: false,
            skill_inputs: Vec::new(),
            mcp_servers: Vec::new(),
            tools: Vec::new(),
            input_parts: Vec::new(),
            snapshot_root: None,
            max_context_messages: None,
            selected_capability_roots: Vec::new(),
            workspace_mutations: None,
            initial_thread_preferences: BTreeMap::new(),
            admission_mission: None,
            target: AgentTargetSelection::default(),
            requested_turn_id: None,
            admission_cancellation: None,
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
        self.target.runtime_profile_ref = runtime_ref;
        self.target.runtime_options = runtime_options;
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
        self.target.agent_ref = agent;
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

    pub fn with_input_parts(mut self, input_parts: Vec<AgentInputPart>) -> Self {
        self.input_parts = input_parts;
        self
    }

    pub fn with_runtime_tools(mut self, tools: Vec<RuntimeTool>) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_framework_context(
        mut self,
        snapshot_root: Option<PathBuf>,
        max_context_messages: Option<usize>,
        selected_capability_roots: Vec<crate::extensions::SelectedCapabilityRoot>,
        workspace_mutations: Option<crate::types::WorkspaceMutationSink>,
    ) -> Self {
        self.snapshot_root = snapshot_root;
        self.max_context_messages = max_context_messages;
        self.selected_capability_roots = selected_capability_roots;
        self.workspace_mutations = workspace_mutations;
        self
    }

    pub fn with_initial_thread_preferences(
        mut self,
        preferences: BTreeMap<String, String>,
    ) -> Self {
        self.initial_thread_preferences = preferences;
        self
    }

    pub fn with_admission_mission(mut self, mission: AgentMissionRegistration) -> Self {
        self.admission_mission = Some(mission);
        self
    }

    pub fn with_agent_preparation(mut self, preparation: AgentPreparationToken) -> Self {
        self.target.preparation = Some(preparation);
        self
    }

    pub fn with_agent_target_expectation(
        mut self,
        profile_revision: Option<u64>,
        backend_ref: Option<String>,
    ) -> Self {
        self.target.expected_profile_revision = profile_revision;
        self.target.expected_backend_ref = backend_ref;
        self
    }

    pub fn with_requested_turn_id(mut self, turn_id: String) -> Self {
        self.requested_turn_id = Some(turn_id);
        self
    }

    pub fn with_admission_cancellation(mut self, cancellation: TurnAdmissionCancellation) -> Self {
        self.admission_cancellation = Some(cancellation);
        self
    }

    pub fn tool(mut self, tool: Arc<dyn psychevo_agent_core::ToolBinding>) -> Self {
        self.tools.push(RuntimeTool::new(tool));
        self
    }

    pub(super) fn resolve(
        self,
        inherited_env: BTreeMap<String, String>,
        application_config_path: Option<PathBuf>,
    ) -> ResolvedTurnPlan {
        let parts = if self.input_parts.is_empty() {
            let mut parts = Vec::with_capacity(1 + self.image_inputs.len());
            if !self.prompt.is_empty() {
                parts.push(AgentInputPart::Text {
                    text: self.prompt.clone(),
                });
            }
            parts.extend(
                self.image_inputs
                    .iter()
                    .cloned()
                    .map(|input| AgentInputPart::Image { input }),
            );
            parts
        } else {
            self.input_parts
        };
        ResolvedTurnPlan {
            client_turn_id: self.client_turn_id,
            requested_turn_id: self.requested_turn_id,
            initial_thread_preferences: self.initial_thread_preferences,
            admission_mission: self.admission_mission,
            target: self.target,
            input: AgentTurnInput {
                prompt: self.prompt,
                image_inputs: self.image_inputs,
                parts,
                extract_prompt_image_sources: self.extract_prompt_image_sources,
                prompt_display: self.prompt_display,
            },
            model: AgentModelSelection {
                model: self.model,
                reasoning_effort: self.reasoning_effort,
                include_reasoning: self.include_reasoning,
            },
            execution: AgentExecutionPolicy {
                source: self.source,
                config_path: self.config_path.or(application_config_path),
                mode: self.mode,
                permission_mode: self.permission_mode,
                approval_handler: self.approval_handler,
                clarify_enabled: self.clarify_enabled,
                project_context: self.project_context,
                sandbox: self.sandbox,
                snapshot_root: self.snapshot_root,
                max_context_messages: self.max_context_messages,
                workspace_mutations: self.workspace_mutations,
            },
            capabilities: ResolvedCapabilityPlan {
                no_agents: self.no_agents,
                no_skills: self.no_skills,
                selected_capability_roots: self.selected_capability_roots,
                skill_inputs: self.skill_inputs,
                mcp_servers: self.mcp_servers,
                tools: self.tools,
            },
            environment: AgentEnvironmentOverlay { inherited_env },
            admission_cancellation: self.admission_cancellation,
        }
    }
}
