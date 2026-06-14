impl PermissionRuntime {
    pub(crate) fn new(
        workdir: PathBuf,
        project_config_dir: PathBuf,
        config: PermissionConfig,
        mode: PermissionMode,
        _approval_mode: ApprovalMode,
        approval_handler: Option<Arc<dyn crate::types::ApprovalHandler>>,
        smart_approval_handler: Option<Arc<dyn crate::types::ApprovalHandler>>,
    ) -> Self {
        Self {
            inner: Arc::new(PermissionRuntimeInner {
                workdir,
                project_config_dir,
                mode,
                config,
                sandbox_policy: crate::sandbox::SandboxPolicy::disabled(),
                sandbox_grants: crate::sandbox::SandboxWriteGrants::default(),
                session_grants: Mutex::new(HashSet::new()),
                pending_approvals: Mutex::new(VecDeque::new()),
                approval_events: Mutex::new(Vec::new()),
                approval_handler,
                smart_approval_handler,
            }),
        }
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
        transport: &str,
    ) -> std::result::Result<(), String> {
        let args = json!({
            "server": server,
            "transport": transport,
        });
        self.authorize(&format!("mcp_startup:{server}"), "mcp_startup", &args)
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

    pub(crate) async fn authorize(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        args: &Value,
    ) -> std::result::Result<(), ToolOutput> {
        self.authorize_inner(tool_call_id, tool_name, args, None)
            .await
    }

    pub(crate) async fn authorize_with_abort(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        args: &Value,
        abort: AbortSignal,
    ) -> std::result::Result<(), ToolOutput> {
        self.authorize_inner(tool_call_id, tool_name, args, Some(abort))
            .await
    }

    pub(crate) async fn authorize_inner(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        args: &Value,
        abort: Option<AbortSignal>,
    ) -> std::result::Result<(), ToolOutput> {
        if abort.as_ref().is_some_and(AbortSignal::aborted) {
            return Err(ToolOutput::error("aborted"));
        }
        let action = PermissionAction::from_tool_call(&self.inner.workdir, tool_name, args);
        match self.evaluate(tool_name, args) {
            PermissionDecision::Allow => {
                let sandbox_grant = match action.as_ref() {
                    Some(action) => self.sandbox_write_grant_request(action)?,
                    None => None,
                };
                if let Some(grant) = sandbox_grant {
                    let session_key = action
                        .as_ref()
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
                    return self
                        .authorize_sandbox_write_grant(
                            tool_call_id,
                            tool_name,
                            args,
                            session_key,
                            grant,
                            abort,
                        )
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
                let sandbox_grant = match action.as_ref() {
                    Some(action) => self.sandbox_write_grant_request(action)?,
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
                        abort,
                    })
                    .await?;
                if self.inner.config.approvals_reviewer == ApprovalsReviewer::Smart {
                    return match decision.outcome {
                        PermissionApprovalOutcome::AllowOnce
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
                    PermissionApprovalOutcome::AllowSession => {
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
}
