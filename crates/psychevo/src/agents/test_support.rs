use super::{
    catalog_surface::*, child_runs::*, definition_policy::*, lifecycle::*, mailbox_tools::*,
    teams::*, *,
};

#[cfg(test)]
mod tests {
    use super::*;
    pub(crate) use futures::future::BoxFuture;
    pub(crate) use psychevo_agent_core::{
        AssistantBlock, ToolBinding, ToolCallBlock, ToolExecutionMode, ToolOutput,
    };
    pub(crate) use psychevo_ai::{
        AbortSignal, AdapterCall, AdapterFuture, AdapterStream, DeploymentConfig, Fake,
        FakeLanguageAdapter, LanguageAdapter, LanguageAdapterEvent, LanguageRequest, Provider,
    };
    pub(crate) use tempfile::TempDir;
    pub(crate) use tokio::sync::watch;

    #[derive(Debug, Clone)]
    pub(crate) enum RawStreamEvent {
        Text(String),
        ToolStart {
            content_index: usize,
            call_index: usize,
            id: String,
            name: String,
        },
        ToolArgs {
            content_index: usize,
            call_index: usize,
            delta: String,
        },
        ToolEnd {
            content_index: usize,
            call_index: usize,
        },
        Done,
    }

    struct TestTool(&'static str);

    impl ToolBinding for TestTool {
        fn name(&self) -> &str {
            self.0
        }

        fn description(&self) -> &str {
            "test tool"
        }

        fn parameters(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }

        fn execution_mode(&self) -> ToolExecutionMode {
            ToolExecutionMode::Parallel
        }

        fn execute(
            &self,
            _tool_call_id: String,
            _args: Value,
            _abort: AbortSignal,
        ) -> BoxFuture<'static, ToolOutput> {
            Box::pin(async { ToolOutput::ok(json!({})) })
        }
    }

    pub(crate) fn test_tool(name: &'static str) -> Arc<dyn ToolBinding> {
        Arc::new(TestTool(name))
    }

    pub(crate) fn test_language_model(adapter: impl LanguageAdapter) -> Provider {
        Provider::builder(
            DeploymentConfig::new("fake", "fake", "fake://local")
                .with_default_language_protocol("fake"),
        )
        .language_adapter(adapter)
        .build()
        .expect("fake provider")
    }

    pub(crate) fn fake_language_model(scripts: Vec<Vec<RawStreamEvent>>) -> Provider {
        let scripts = scripts
            .into_iter()
            .map(raw_language_script)
            .map(|events| events.into_iter().map(Ok).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        Fake::with_language(FakeLanguageAdapter::new(scripts))
            .expect("built-in fake provider")
            .provider()
    }

    fn raw_language_script(events: Vec<RawStreamEvent>) -> Vec<LanguageAdapterEvent> {
        let mut normalized = Vec::new();
        let mut text_index = None;
        let mut next_content_index = 0;
        let mut arguments = BTreeMap::<(usize, usize), String>::new();
        for event in events {
            match event {
                RawStreamEvent::Text(text) => {
                    let content_index = *text_index.get_or_insert_with(|| {
                        let content_index = next_content_index;
                        next_content_index += 1;
                        normalized.push(LanguageAdapterEvent::TextStart { content_index });
                        content_index
                    });
                    normalized.push(LanguageAdapterEvent::TextDelta {
                        content_index,
                        delta: text,
                    });
                }
                RawStreamEvent::ToolStart {
                    content_index,
                    call_index,
                    id,
                    name,
                } => {
                    arguments.insert((content_index, call_index), String::new());
                    normalized.push(LanguageAdapterEvent::ToolCallStart {
                        content_index,
                        id,
                        name,
                    });
                    next_content_index = next_content_index.max(content_index + 1);
                }
                RawStreamEvent::ToolArgs {
                    content_index,
                    call_index,
                    delta,
                } => {
                    arguments
                        .get_mut(&(content_index, call_index))
                        .expect("tool arguments after start")
                        .push_str(&delta);
                    normalized.push(LanguageAdapterEvent::ToolCallArgumentsDelta {
                        content_index,
                        delta,
                    });
                }
                RawStreamEvent::ToolEnd {
                    content_index,
                    call_index,
                } => normalized.push(LanguageAdapterEvent::ToolCallEnd {
                    content_index,
                    arguments_raw: arguments
                        .remove(&(content_index, call_index))
                        .expect("tool end after start"),
                }),
                RawStreamEvent::Done => {
                    if let Some(content_index) = text_index.take() {
                        normalized.push(LanguageAdapterEvent::TextEnd { content_index });
                    }
                    normalized.push(LanguageAdapterEvent::Finish {
                        finish_reason: None,
                    });
                }
            }
        }
        normalized
    }

    pub(crate) fn test_agent_run_record(
        parent_session_id: String,
        child_session_id: Option<String>,
    ) -> AgentRunRecord {
        AgentRunRecord {
            id: "agent-1".to_string(),
            task_name: Some("worker-task".to_string()),
            agent_name: "worker".to_string(),
            task: "do the work".to_string(),
            parent_session_id,
            child_session_id,
            role: AgentInvocationRole::Subagent,
            background: true,
            status: AgentRunStatus::Completed,
            edge_status: Some(AgentEdgeStatus::Open),
            started_at_ms: 1,
            ended_at_ms: Some(2),
            outcome: Some("normal".to_string()),
            final_answer: Some("mailbox final".to_string()),
            error: None,
            effective_max_spawn_depth: Some(0),
            team_run_id: None,
            mission_run_id: None,
            team_name: None,
            team_member_id: None,
            agent_path: None,
        }
    }

    pub(crate) fn env(home: &Path) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("HOME".to_string(), home.display().to_string()),
            (
                "PSYCHEVO_HOME".to_string(),
                home.join(".psychevo").display().to_string(),
            ),
        ])
    }

    pub(crate) fn test_agent_tool_context(
        tmp: &TempDir,
        provider: Provider,
        store: StateRuntime,
        _db_path: PathBuf,
        parent: String,
        catalog: AgentCatalog,
    ) -> AgentToolContext {
        AgentToolContext {
            provider,
            model_provider: "provider".to_string(),
            model: "model".to_string(),
            provider_label: "provider".to_string(),
            base_url: "http://127.0.0.1:9/v1".to_string(),
            api_key_env: None,
            reasoning_effort: None,
            context_limit: None,
            generation_metadata: json!({}),
            cwd: tmp.path().to_path_buf(),
            mode: RunMode::Default,
            project_context_mode: Default::default(),
            permission_config: PermissionConfig::default(),
            lsp: Default::default(),
            permission_mode: PermissionMode::Default,
            approval_handler: None,
            state: store,
            config_path: None,
            protected_config_paths: Vec::new(),
            parent_session_id: parent,
            parent_context_snapshot: Vec::new(),
            catalog,
            control_handle: None,
            stream_events: None,
            workspace_mutations: None,
            model_metadata: ModelMetadata::default(),
            env: BTreeMap::new(),
            path_prefixes: Vec::new(),
            sandbox_policy: crate::sandbox::SandboxPolicy::disabled(),
            home: tmp.path().join(".psychevo"),
            image_input_enabled: true,
            image_generation: None,
            web_search: Default::default(),
            tool_selection: Default::default(),
            custom_toolsets: BTreeMap::new(),
            extension_inputs: Default::default(),
            allowed_agent_names: None,
            denied_agent_names: BTreeSet::new(),
            required_agent_names: Vec::new(),
            spawn_depth_remaining: None,
            active_team: None,
            external_delegate: None,
            supervisor: AgentSupervisor::default(),
        }
    }

    pub(crate) fn assert_first_party_tool_declaration_quality(tool: &dyn ToolBinding) {
        crate::tests::assert_first_party_tool_declaration_quality(tool);
    }

    pub(crate) mod catalog_and_lifecycle;
    pub(crate) mod policy_and_control;
}
