#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActionPolicyEvaluation {
    Allow,
    Ask {
        reason: String,
        matched_rule: Option<String>,
        suggested_rule: Option<String>,
        persistent_grants: Vec<PersistentPermissionGrant>,
    },
    Deny {
        reason: String,
        matched_rule: Option<String>,
    },
}

pub(crate) struct PermissionTool {
    pub(crate) tool: Arc<dyn ToolBinding>,
    pub(crate) runtime: PermissionRuntime,
}

impl ToolBinding for PermissionTool {
    fn name(&self) -> &str {
        self.tool.name()
    }

    fn description(&self) -> &str {
        self.tool.description()
    }

    fn parameters(&self) -> Value {
        self.tool.parameters()
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        self.tool.execution_mode()
    }

    fn display_spec(&self) -> ToolDisplaySpec {
        self.tool.display_spec()
    }

    fn execute(
        &self,
        tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let runtime = self.runtime.clone();
        let tool = Arc::clone(&self.tool);
        Box::pin(async move {
            if let Err(output) = runtime
                .authorize_with_abort(&tool_call_id, tool.name(), &args, abort.clone())
                .await
            {
                return output;
            }
            tool.execute(tool_call_id, args, abort).await
        })
    }
}
