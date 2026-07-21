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

    fn canonical_tool_name(&self) -> psychevo_ai::ToolName {
        self.tool.canonical_tool_name()
    }

    fn description(&self) -> &str {
        self.tool.description()
    }

    fn parameters(&self) -> Value {
        self.tool.parameters()
    }

    fn exposure(&self) -> psychevo_agent_core::ToolExposure {
        self.tool.exposure()
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
            let approved_identity = match PermissionAction::from_tool_call(
                &runtime.inner.cwd,
                tool.name(),
                &args,
            ) {
                Ok(action) => action.and_then(|action| action.filesystem_identity_snapshot()),
                Err(err) => {
                    return ToolOutput::error(format!(
                        "filesystem identity resolution failed: {err}"
                    ));
                }
            };
            if let Err(output) = runtime
                .authorize_with_expected_identity(
                    &tool_call_id,
                    tool.name(),
                    &args,
                    abort.clone(),
                    &approved_identity,
                )
                .await
            {
                return output;
            }
            let current_identity = match PermissionAction::from_tool_call(
                &runtime.inner.cwd,
                tool.name(),
                &args,
            ) {
                Ok(action) => action.and_then(|action| action.filesystem_identity_snapshot()),
                Err(err) => {
                    return ToolOutput::error(format!(
                        "path_identity_changed: filesystem identity could not be revalidated: {err}"
                    ));
                }
            };
            if current_identity != approved_identity {
                return ToolOutput::error(
                    "path_identity_changed: filesystem identity changed after permission evaluation",
                );
            }
            tool.execute(tool_call_id, args, abort).await
        })
    }
}
