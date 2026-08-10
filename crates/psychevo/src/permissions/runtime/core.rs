use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use psychevo_agent_core::{ToolBinding, ToolOutput};
use psychevo_ai::AbortSignal;
use serde_json::{Value, json};
use sha2::Digest;

use super::super::rules::{action_summary, permission_error};
use super::actions::PermissionAction;
use super::state::{
    ApprovalDecisionRequest, PermissionDecision, PermissionRuntime, PermissionRuntimeInner,
};
use super::tool::PermissionTool;
use crate::types::{
    ApprovalsReviewer, PermissionApprovalOutcome, PermissionConfig, PermissionMode,
};

#[derive(Clone, Copy)]
struct AuthorizationIdentityCheck<'a> {
    expected: Option<&'a Option<Vec<PathBuf>>>,
    roots_prevalidated: bool,
}

impl PermissionRuntime {
    #[cfg(test)]
    pub(crate) fn has_smart_approval_handler(&self) -> bool {
        self.inner.smart_approval_handler.is_some()
    }

    pub(crate) fn update_cache_identity_hasher(&self, hasher: &mut sha2::Sha256) {
        fn update_value(hasher: &mut sha2::Sha256, value: &str) {
            hasher.update(value.len().to_le_bytes());
            hasher.update(value.as_bytes());
        }

        update_value(hasher, &self.inner.cwd.to_string_lossy());
        for root in &self.inner.workspace_roots {
            update_value(hasher, &root.to_string_lossy());
        }
        update_value(hasher, &self.inner.project_config_dir.to_string_lossy());
        update_value(hasher, self.inner.mode.as_str());
        update_value(hasher, &format!("{:?}", self.inner.config));
        update_value(hasher, &format!("{:?}", self.inner.sandbox_policy));
        for path in &self.inner.protected_config_paths {
            update_value(hasher, &path.to_string_lossy());
        }
        hasher.update([
            u8::from(self.inner.approval_handler.is_some()),
            u8::from(self.inner.smart_approval_handler.is_some()),
            u8::from(self.inner.hook_runtime.is_some()),
        ]);
    }

    pub(crate) fn new(
        cwd: PathBuf,
        project_config_dir: PathBuf,
        config: PermissionConfig,
        mode: PermissionMode,
        approval_handler: Option<Arc<dyn crate::types::ApprovalHandler>>,
        smart_approval_handler: Option<Arc<dyn crate::types::ApprovalHandler>>,
    ) -> Self {
        let protected_config_paths = crate::filesystem_identity::canonicalize_deepest_existing(
            &project_config_dir.join("config.toml"),
        )
        .into_iter()
        .collect();
        Self {
            inner: Arc::new(PermissionRuntimeInner {
                workspace_roots: vec![cwd.clone()],
                workspace_root_identities:
                    crate::filesystem_identity::CapturedDirectoryIdentity::capture(&cwd)
                        .into_iter()
                        .collect(),
                cwd,
                project_config_dir,
                protected_config_paths,
                mode,
                config,
                sandbox_policy: crate::sandbox::SandboxPolicy::disabled(),
                sandbox_grants: crate::sandbox::SandboxWriteGrants::default(),
                session_grants: Mutex::new(HashSet::new()),
                pending_approvals: Mutex::new(VecDeque::new()),
                approval_events: Mutex::new(Vec::new()),
                approval_handler,
                smart_approval_handler,
                hook_runtime: None,
            }),
        }
    }

    pub(crate) fn with_workspace_roots(
        mut self,
        roots: impl IntoIterator<Item = PathBuf>,
    ) -> crate::error::Result<Self> {
        let inner = Arc::get_mut(&mut self.inner)
            .expect("Workspace roots must be attached before PermissionRuntime is cloned");
        let mut identities =
            vec![crate::filesystem_identity::CapturedDirectoryIdentity::capture(&inner.cwd)?];
        for root in roots {
            let identity = crate::filesystem_identity::CapturedDirectoryIdentity::capture(&root)?;
            if !identities
                .iter()
                .any(|current| current.path() == identity.path())
            {
                identities.push(identity);
            }
        }
        inner.workspace_roots = identities
            .iter()
            .map(|identity| identity.path().to_path_buf())
            .collect();
        inner.workspace_root_identities = identities;
        Ok(self)
    }

    pub(crate) fn with_workspace_root_capture(
        mut self,
        capture: &crate::filesystem_identity::WorkspaceRootCapture,
    ) -> Self {
        let inner = Arc::get_mut(&mut self.inner)
            .expect("Workspace roots must be attached before PermissionRuntime is cloned");
        inner.workspace_roots = capture.paths();
        inner.workspace_root_identities = capture.identities().to_vec();
        self
    }

    pub(super) fn validate_workspace_root_identities(&self) -> crate::error::Result<()> {
        for identity in &self.inner.workspace_root_identities {
            identity.validate()?;
        }
        Ok(())
    }

    pub(crate) fn open_captured_workspace_directory(
        &self,
        target: &std::path::Path,
    ) -> crate::error::Result<std::fs::File> {
        crate::filesystem_identity::WorkspaceRootCapture::from_identities(
            self.inner.workspace_root_identities.clone(),
        )
        .open_directory(target)
    }

    pub(crate) fn with_protected_config_paths(
        mut self,
        paths: impl IntoIterator<Item = PathBuf>,
    ) -> Self {
        let inner = Arc::get_mut(&mut self.inner)
            .expect("protected paths must be attached before PermissionRuntime is cloned");
        for path in paths {
            if let Ok(path) = crate::filesystem_identity::canonicalize_deepest_existing(&path)
                && !inner.protected_config_paths.contains(&path)
            {
                inner.protected_config_paths.push(path);
            }
        }
        self
    }

    pub(crate) fn with_hook_runtime(mut self, hook_runtime: crate::hooks::HookRuntime) -> Self {
        let inner = Arc::get_mut(&mut self.inner)
            .expect("hook runtime must be attached before PermissionRuntime is cloned");
        inner.hook_runtime = Some(hook_runtime);
        self
    }

    pub(crate) fn with_sandbox(
        mut self,
        sandbox_policy: crate::sandbox::SandboxPolicy,
        sandbox_grants: crate::sandbox::SandboxWriteGrants,
    ) -> Self {
        let inner = Arc::get_mut(&mut self.inner)
            .expect("sandbox must be attached before PermissionRuntime is cloned");
        inner.sandbox_policy = sandbox_policy;
        inner.sandbox_grants = sandbox_grants;
        self
    }

    pub(crate) fn wrap_tools(&self, tools: Vec<Arc<dyn ToolBinding>>) -> Vec<Arc<dyn ToolBinding>> {
        tools
            .into_iter()
            .map(|tool| {
                Arc::new(PermissionTool {
                    tool,
                    runtime: self.clone(),
                }) as Arc<dyn ToolBinding>
            })
            .collect()
    }

    pub(crate) async fn authorize_mcp_startup(
        &self,
        server: &str,
        source: &str,
        target: &crate::types::McpStartupApprovalTarget,
        descriptor_fingerprint: &str,
    ) -> std::result::Result<(), String> {
        let args = json!({
            "server": server,
            "source": source,
            "target": target,
            "descriptorFingerprint": descriptor_fingerprint,
        });
        self.authorize(
            &format!("mcp_startup:{server}@{descriptor_fingerprint}"),
            "mcp_startup",
            &args,
        )
        .await
        .map_err(|output| {
            output
                .json
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("permission denied")
                .to_string()
        })
    }

    pub(crate) async fn cancel_authorization(&self, tool_call_id: &str) {
        let handler = match self.inner.config.approvals_reviewer {
            crate::types::ApprovalsReviewer::User => self.inner.approval_handler.as_ref(),
            crate::types::ApprovalsReviewer::Smart => self.inner.smart_approval_handler.as_ref(),
        };
        if let Some(handler) = handler {
            handler.cancel_permission(tool_call_id).await;
        }
    }

    pub(crate) async fn authorize(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        args: &Value,
    ) -> std::result::Result<(), ToolOutput> {
        self.authorize_inner(tool_call_id, tool_name, args, None, None)
            .await
    }

    pub(crate) async fn authorize_filesystem_callback_target(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        path: &std::path::Path,
        writable: bool,
        abort: AbortSignal,
        require_workspace_containment: bool,
    ) -> std::result::Result<crate::filesystem_identity::CapturedFileTarget, ToolOutput> {
        let args = json!({ "path": path });
        let prepared_runtime = self.clone();
        let prepared_args = args.clone();
        let prepared_tool_name = tool_name.to_string();
        let (approved_action, approved_identity, captured_target) =
            tokio::task::spawn_blocking(move || {
                let approved_action = PermissionAction::from_tool_call(
                    &prepared_runtime.inner.cwd,
                    &prepared_runtime.inner.workspace_roots,
                    &prepared_tool_name,
                    &prepared_args,
                )
                .map_err(|err| format!("filesystem identity resolution failed: {err}"))?;
                let approved_identity = approved_action
                    .as_ref()
                    .and_then(PermissionAction::filesystem_identity_snapshot)
                    .ok_or_else(|| "filesystem permission action is unavailable".to_string())?;
                let target = approved_identity
                    .first()
                    .ok_or_else(|| "filesystem permission target is unavailable".to_string())?;
                prepared_runtime
                    .validate_workspace_identity_for_target(target, require_workspace_containment)
                    .map_err(|error| error.to_string())?;
                let captured_target =
                    crate::filesystem_identity::CapturedFileTarget::capture(target, writable)
                        .map_err(|error| error.to_string())?;
                Ok::<_, String>((approved_action, approved_identity, captured_target))
            })
            .await
            .map_err(|error| {
                ToolOutput::error(format!("filesystem identity worker failed: {error}"))
            })?
            .map_err(ToolOutput::error)?;
        let approved_identity_expectation = Some(approved_identity.clone());
        self.authorize_resolved_inner(
            tool_call_id,
            tool_name,
            &args,
            approved_action.as_ref(),
            Some(abort),
            AuthorizationIdentityCheck {
                expected: Some(&approved_identity_expectation),
                roots_prevalidated: true,
            },
        )
        .await?;
        let current_runtime = self.clone();
        let current_tool_name = tool_name.to_string();
        tokio::task::spawn_blocking(move || {
            let target = approved_identity
                .first()
                .ok_or_else(|| "filesystem permission target is unavailable".to_string())?;
            current_runtime
                .validate_workspace_identity_for_target(target, require_workspace_containment)
                .map_err(|error| error.to_string())?;
            let current_identity = PermissionAction::from_tool_call(
                &current_runtime.inner.cwd,
                &current_runtime.inner.workspace_roots,
                &current_tool_name,
                &args,
            )
            .map_err(|err| {
                format!(
                    "path_identity_changed: filesystem identity could not be revalidated: {err}"
                )
            })?
            .and_then(|action| action.filesystem_identity_snapshot())
            .ok_or_else(|| "filesystem permission action is unavailable".to_string())?;
            if current_identity != approved_identity {
                return Err(
                    "path_identity_changed: filesystem identity changed after permission evaluation"
                        .to_string(),
                );
            }
            captured_target
                .revalidate()
                .map_err(|error| error.to_string())?;
            Ok(captured_target)
        })
        .await
        .map_err(|error| ToolOutput::error(format!("filesystem identity worker failed: {error}")))?
        .map_err(ToolOutput::error)
    }

    #[cfg(test)]
    pub(crate) async fn authorize_with_abort(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        args: &Value,
        abort: AbortSignal,
    ) -> std::result::Result<(), ToolOutput> {
        self.authorize_inner(tool_call_id, tool_name, args, Some(abort), None)
            .await
    }

    pub(super) async fn authorize_with_expected_identity(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        args: &Value,
        action: &Option<PermissionAction>,
        abort: AbortSignal,
        expected_identity: &Option<Vec<PathBuf>>,
    ) -> std::result::Result<(), ToolOutput> {
        self.authorize_resolved_inner(
            tool_call_id,
            tool_name,
            args,
            action.as_ref(),
            Some(abort),
            AuthorizationIdentityCheck {
                expected: Some(expected_identity),
                roots_prevalidated: false,
            },
        )
        .await
    }

    async fn authorize_inner(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        args: &Value,
        abort: Option<AbortSignal>,
        expected_identity: Option<&Option<Vec<PathBuf>>>,
    ) -> std::result::Result<(), ToolOutput> {
        let action = PermissionAction::from_tool_call(
            &self.inner.cwd,
            &self.inner.workspace_roots,
            tool_name,
            args,
        )
        .map_err(|err| {
            permission_error(
                "denied",
                &format!("filesystem identity resolution failed: {err}"),
                None,
            )
        })?;
        self.authorize_resolved_inner(
            tool_call_id,
            tool_name,
            args,
            action.as_ref(),
            abort,
            AuthorizationIdentityCheck {
                expected: expected_identity,
                roots_prevalidated: false,
            },
        )
        .await
    }

    async fn authorize_resolved_inner(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        args: &Value,
        action: Option<&PermissionAction>,
        abort: Option<AbortSignal>,
        identity_check: AuthorizationIdentityCheck<'_>,
    ) -> std::result::Result<(), ToolOutput> {
        if abort.as_ref().is_some_and(AbortSignal::aborted) {
            return Err(ToolOutput::error("aborted"));
        }
        if !identity_check.roots_prevalidated
            && action
                .and_then(PermissionAction::filesystem_identity_snapshot)
                .is_some()
        {
            self.validate_workspace_root_identities()
                .map_err(|error| ToolOutput::error(error.to_string()))?;
        }
        if let Some(expected_identity) = identity_check.expected
            && action.and_then(PermissionAction::filesystem_identity_snapshot) != *expected_identity
        {
            return Err(ToolOutput::error(
                "path_identity_changed: filesystem identity changed before permission evaluation",
            ));
        }
        match self.evaluate_resolved_action(action) {
            PermissionDecision::Allow => {
                let sandbox_grant = match action {
                    Some(action) => self
                        .sandbox_write_grant_request(action)
                        .map_err(ToolOutput::error)?,
                    None => None,
                };
                if let Some(grant) = sandbox_grant {
                    let session_key =
                        action
                            .map(PermissionAction::session_key)
                            .unwrap_or_else(|| {
                                format!("{tool_name}:{}", action_summary(tool_name, args))
                            });
                    if self
                        .inner
                        .sandbox_grants
                        .grant_call_from_session(tool_call_id, &session_key)
                    {
                        return Ok(());
                    }
                    if self
                        .inner
                        .sandbox_grants
                        .grant_call_from_scopes(tool_call_id, &grant.paths)
                        .map_err(|err| ToolOutput::error(err.to_string()))?
                    {
                        return Ok(());
                    }
                    return self
                        .authorize_sandbox_write_grant(tool_call_id, tool_name, args, grant, abort)
                        .await;
                }
                Ok(())
            }
            PermissionDecision::Deny {
                reason,
                matched_rule,
            } => Err(permission_error("denied", &reason, matched_rule.as_deref())),
            PermissionDecision::Ask {
                reason,
                matched_rule,
                suggested_rule,
                allow_always,
                session_key,
                persistent_grants,
            } => {
                let sandbox_grant = match action {
                    Some(action) => self
                        .sandbox_write_grant_request(action)
                        .map_err(ToolOutput::error)?,
                    None => None,
                };
                if self.inner.mode.bypasses_prompt_asks() {
                    if let Some(grant) = sandbox_grant {
                        return Err(ToolOutput::error(format!(
                            "denied by sandbox policy: {}; bypassPermissions does not bypass sandbox enforcement",
                            grant.reason
                        )));
                    }
                    return Ok(());
                }
                let approval_reason = if let Some(grant) = &sandbox_grant {
                    format!("{reason}; sandbox approval required: {}", grant.reason)
                } else {
                    reason.clone()
                };
                let decision = self
                    .request_approval_decision(ApprovalDecisionRequest {
                        tool_call_id,
                        tool_name,
                        args,
                        reason: &approval_reason,
                        matched_rule: matched_rule.as_deref(),
                        suggested_rule: suggested_rule.clone(),
                        allow_always: allow_always && sandbox_grant.is_none(),
                        filesystem: action.and_then(PermissionAction::filesystem_approval_request),
                        mcp_startup: action
                            .and_then(PermissionAction::mcp_startup_approval_request),
                        abort,
                    })
                    .await?;
                if self.inner.config.approvals_reviewer == ApprovalsReviewer::Smart {
                    return match decision.outcome {
                        PermissionApprovalOutcome::AllowOnce
                        | PermissionApprovalOutcome::AllowTurn
                        | PermissionApprovalOutcome::AllowSession
                        | PermissionApprovalOutcome::AllowAlways => {
                            if let Some(grant) = &sandbox_grant {
                                self.inner
                                    .sandbox_grants
                                    .grant_once(tool_call_id, &grant.paths)
                                    .map_err(|err| ToolOutput::error(err.to_string()))?;
                            }
                            Ok(())
                        }
                        PermissionApprovalOutcome::Deny => Err(permission_error(
                            "denied",
                            &format!("smart reviewer denied permission: {approval_reason}"),
                            matched_rule.as_deref(),
                        )),
                    };
                }
                match decision.outcome {
                    PermissionApprovalOutcome::AllowOnce => {
                        if let Some(grant) = &sandbox_grant {
                            self.inner
                                .sandbox_grants
                                .grant_once(tool_call_id, &grant.paths)
                                .map_err(|err| ToolOutput::error(err.to_string()))?;
                        }
                        Ok(())
                    }
                    PermissionApprovalOutcome::AllowTurn => {
                        let scope = decision
                            .filesystem_scope
                            .as_ref()
                            .expect("validated filesystem turn scope");
                        self.remember_filesystem_scope(scope)
                            .map_err(|err| ToolOutput::error(err.to_string()))?;
                        if let Some(grant) = &sandbox_grant {
                            self.inner
                                .sandbox_grants
                                .grant_once(tool_call_id, &grant.paths)
                                .map_err(|err| ToolOutput::error(err.to_string()))?;
                        }
                        Ok(())
                    }
                    PermissionApprovalOutcome::AllowSession => {
                        if let Some(scope) = &decision.filesystem_scope {
                            self.remember_filesystem_scope(scope)
                                .map_err(|err| ToolOutput::error(err.to_string()))?;
                        } else {
                            self.remember_session_grant(session_key.clone());
                        }
                        if let Some(grant) = &sandbox_grant {
                            self.inner
                                .sandbox_grants
                                .grant_once(tool_call_id, &grant.paths)
                                .map_err(|err| ToolOutput::error(err.to_string()))?;
                            if decision.filesystem_scope.is_none() {
                                self.inner
                                    .sandbox_grants
                                    .grant_session(&session_key, &grant.paths)
                                    .map_err(|err| ToolOutput::error(err.to_string()))?;
                            }
                        }
                        Ok(())
                    }
                    PermissionApprovalOutcome::AllowAlways => {
                        self.remember_session_grant(session_key.clone());
                        if let Some(grant) = &sandbox_grant {
                            self.inner
                                .sandbox_grants
                                .grant_once(tool_call_id, &grant.paths)
                                .map_err(|err| ToolOutput::error(err.to_string()))?;
                            self.inner
                                .sandbox_grants
                                .grant_session(&session_key, &grant.paths)
                                .map_err(|err| ToolOutput::error(err.to_string()))?;
                        } else if allow_always {
                            self.persist_permission_grants(&persistent_grants);
                        }
                        Ok(())
                    }
                    PermissionApprovalOutcome::Deny => Err(permission_error(
                        "denied",
                        &format!(
                            "user denied permission; do not retry the same operation: {approval_reason}"
                        ),
                        matched_rule.as_deref(),
                    )),
                }
            }
        }
    }

    fn validate_workspace_identity_for_target(
        &self,
        target: &std::path::Path,
        required: bool,
    ) -> crate::error::Result<()> {
        let identity = self
            .inner
            .workspace_root_identities
            .iter()
            .filter(|identity| crate::filesystem_identity::is_within(identity.path(), target))
            .max_by_key(|identity| identity.path().components().count());
        let Some(identity) = identity else {
            if !required {
                return Ok(());
            }
            return Err(crate::Error::Message(format!(
                "filesystem target is outside the captured Workspace: {}",
                target.display()
            )));
        };
        identity.validate()
    }
}
