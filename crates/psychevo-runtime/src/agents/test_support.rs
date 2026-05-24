#[allow(unused_imports)]
pub(crate) use super::*;

#[allow(unused_imports)]
pub(crate) use super::*;

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::*;
    pub(crate) use futures::future::BoxFuture;
    pub(crate) use psychevo_agent_core::{
        AssistantBlock, ToolBinding, ToolCallBlock, ToolExecutionMode, ToolOutput,
    };
    pub(crate) use psychevo_ai::{AbortSignal, FakeProvider, RawStreamEvent};
    pub(crate) use tempfile::TempDir;
    pub(crate) use tokio::sync::watch;

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
        provider: Arc<dyn GenerationProvider>,
        store: SqliteStore,
        db_path: PathBuf,
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
            workdir: tmp.path().to_path_buf(),
            mode: RunMode::Default,
            permission_config: PermissionConfig::default(),
            lsp: Default::default(),
            permission_mode: PermissionMode::Default,
            approval_mode: ApprovalMode::Manual,
            approval_handler: None,
            state: StateRuntime::from_store(db_path, store),
            config_path: None,
            parent_session_id: parent,
            parent_context_snapshot: Vec::new(),
            catalog,
            control_handle: None,
            stream_events: None,
            model_metadata: ModelMetadata::default(),
            env: BTreeMap::new(),
            path_prefixes: Vec::new(),
            tool_selection: Default::default(),
            custom_toolsets: BTreeMap::new(),
            allowed_agent_names: None,
            denied_agent_names: BTreeSet::new(),
            required_agent_names: Vec::new(),
            spawn_depth_remaining: None,
        }
    }

    pub(crate) fn assert_tool_schema_descriptions(tool: &dyn ToolBinding) {
        let mut missing = Vec::new();
        collect_missing_schema_descriptions(
            &tool.parameters(),
            tool.name().to_string(),
            &mut missing,
        );
        assert!(
            missing.is_empty(),
            "{} has schema properties without descriptions: {:?}",
            tool.name(),
            missing
        );
    }

    pub(crate) fn collect_missing_schema_descriptions(
        value: &Value,
        path: String,
        missing: &mut Vec<String>,
    ) {
        if let Some(properties) = value.get("properties").and_then(Value::as_object) {
            for (name, property) in properties {
                let property_path = format!("{path}.{name}");
                let described = property
                    .get("description")
                    .and_then(Value::as_str)
                    .is_some_and(|description| !description.trim().is_empty());
                if !described {
                    missing.push(property_path.clone());
                }
                collect_missing_schema_descriptions(property, property_path, missing);
            }
        }
        if let Some(items) = value.get("items") {
            collect_missing_schema_descriptions(items, format!("{path}[]"), missing);
        }
    }

    pub(crate) mod catalog_and_lifecycle;
    pub(crate) mod policy_and_control;
}
