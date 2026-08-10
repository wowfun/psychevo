use std::sync::Arc;

use futures::future::BoxFuture;
use psychevo_agent_core::{ToolBinding, ToolDisplaySpec, ToolExecutionMode, ToolOutput};
use psychevo_ai::AbortSignal;
use serde_json::Value;

use super::actions::PermissionAction;
use super::state::{PermissionRuntime, PersistentPermissionGrant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ActionPolicyEvaluation {
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

pub(super) struct PermissionTool {
    pub(super) tool: Arc<dyn ToolBinding>,
    pub(super) runtime: PermissionRuntime,
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
            let action = match PermissionAction::from_tool_call(
                &runtime.inner.cwd,
                &runtime.inner.workspace_roots,
                tool.name(),
                &args,
            ) {
                Ok(action) => action,
                Err(err) => {
                    return ToolOutput::error(format!(
                        "filesystem identity resolution failed: {err}"
                    ));
                }
            };
            let approved_identity = action
                .as_ref()
                .and_then(PermissionAction::filesystem_identity_snapshot);
            let captured_mutations =
                match crate::filesystem_identity::IDENTITY_BOUND_FILE_MUTATIONS_SUPPORTED
                    .then(|| {
                        action
                            .as_ref()
                            .and_then(PermissionAction::mutating_file_paths)
                            .map(|paths| {
                                paths
                                    .iter()
                                    .map(|path| {
                                        crate::filesystem_identity::CapturedFileTarget::capture(
                                            path, true,
                                        )
                                    })
                                    .collect::<crate::Result<Vec<_>>>()
                            })
                            .transpose()
                    })
                    .transpose()
                    .map(|captured| captured.flatten())
                {
                    Ok(captured) => captured,
                    Err(error) => {
                        return ToolOutput::error(format!(
                            "filesystem identity capture failed: {error}"
                        ));
                    }
                };
            if let Err(output) = runtime
                .authorize_with_expected_identity(
                    &tool_call_id,
                    tool.name(),
                    &args,
                    &action,
                    abort.clone(),
                    &approved_identity,
                )
                .await
            {
                return output;
            }
            if approved_identity.is_some() {
                if let Err(error) = runtime.validate_workspace_root_identities() {
                    return ToolOutput::error(error.to_string());
                }
                let current_identity = match PermissionAction::from_tool_call(
                    &runtime.inner.cwd,
                    &runtime.inner.workspace_roots,
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
            }
            if let Some(captured) = captured_mutations {
                if let Err(error) = captured
                    .iter()
                    .try_for_each(crate::filesystem_identity::CapturedFileTarget::revalidate)
                {
                    return ToolOutput::error(error.to_string());
                }
                runtime
                    .inner
                    .sandbox_grants
                    .install_authorized_files(&tool_call_id, captured);
            }
            let output = tool.execute(tool_call_id.clone(), args, abort).await;
            runtime
                .inner
                .sandbox_grants
                .clear_authorized_files(&tool_call_id);
            output
        })
    }
}
