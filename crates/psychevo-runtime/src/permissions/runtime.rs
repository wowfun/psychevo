#[allow(unused_imports)]
pub(crate) use super::*;

#[derive(Clone)]
pub(crate) struct PermissionRuntime {
    pub(crate) inner: Arc<PermissionRuntimeInner>,
}

pub(crate) struct PermissionRuntimeInner {
    pub(crate) workdir: PathBuf,
    pub(crate) project_config_dir: PathBuf,
    pub(crate) mode: PermissionMode,
    pub(crate) config: PermissionConfig,
    pub(crate) sandbox_policy: crate::sandbox::SandboxPolicy,
    pub(crate) sandbox_grants: crate::sandbox::SandboxWriteGrants,
    pub(crate) session_grants: Mutex<HashSet<String>>,
    pub(crate) pending_approvals: Mutex<VecDeque<String>>,
    pub(crate) approval_events: Mutex<Vec<ApprovalLifecycleEvent>>,
    pub(crate) approval_handler: Option<Arc<dyn crate::types::ApprovalHandler>>,
    pub(crate) smart_approval_handler: Option<Arc<dyn crate::types::ApprovalHandler>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ApprovalLifecycleEvent {
    Requested {
        tool_call_id: String,
        tool_name: String,
    },
    Resolved {
        tool_call_id: String,
        outcome: PermissionApprovalOutcome,
    },
    Aborted {
        tool_call_id: String,
    },
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PermissionRule {
    pub(crate) raw: String,
    pub(crate) tool: String,
    pub(crate) pattern: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PermissionDecision {
    Allow,
    Ask {
        reason: String,
        matched_rule: Option<String>,
        suggested_rule: Option<String>,
        allow_always: bool,
        session_key: String,
        persistent_grants: Vec<PersistentPermissionGrant>,
    },
    Deny {
        reason: String,
        matched_rule: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PersistentPermissionGrant {
    Filesystem {
        path: String,
        access: PermissionAccess,
    },
    Network {
        host: String,
        access: PermissionAccess,
    },
    Exec {
        prefix: Vec<String>,
        decision: ExecPolicyDecision,
    },
    Skill {
        key: String,
        access: PermissionAccess,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SandboxWriteGrantRequest {
    paths: Vec<PathBuf>,
    reason: String,
}

struct ApprovalDecisionRequest<'a> {
    tool_call_id: &'a str,
    tool_name: &'a str,
    args: &'a Value,
    reason: &'a str,
    matched_rule: Option<&'a str>,
    suggested_rule: Option<String>,
    allow_always: bool,
    abort: Option<AbortSignal>,
}

pub(crate) struct PendingApprovalGuard {
    runtime: PermissionRuntime,
    tool_call_id: String,
    finished: bool,
}

impl PendingApprovalGuard {
    pub(crate) fn finish(&mut self, outcome: PermissionApprovalOutcome) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.runtime
            .finish_pending_approval(&self.tool_call_id, Some(outcome));
    }
}

impl Drop for PendingApprovalGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.runtime
                .finish_pending_approval(&self.tool_call_id, None);
        }
    }
}

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

    async fn authorize_sandbox_write_grant(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        args: &Value,
        session_key: String,
        grant: SandboxWriteGrantRequest,
        abort: Option<AbortSignal>,
    ) -> std::result::Result<(), ToolOutput> {
        if self.inner.mode.bypasses_prompt_asks() {
            return Err(ToolOutput::error(format!(
                "denied by sandbox policy: {}; bypassPermissions does not bypass sandbox enforcement",
                grant.reason
            )));
        }
        if let Some(action) = PermissionAction::from_tool_call(&self.inner.workdir, tool_name, args)
            && matches!(self.inner.config.approval_policy, ApprovalPolicy::Granular)
            && !self.granular_allows_prompt(&action)
        {
            return Err(permission_error(
                "denied",
                &format!(
                    "granular approval disabled for filesystem: {}",
                    grant.reason
                ),
                None,
            ));
        }
        let reason = format!("sandbox approval required: {}", grant.reason);
        let decision = self
            .request_approval_decision(ApprovalDecisionRequest {
                tool_call_id,
                tool_name,
                args,
                reason: &reason,
                matched_rule: None,
                suggested_rule: None,
                allow_always: false,
                abort,
            })
            .await?;
        if self.inner.config.approvals_reviewer == ApprovalsReviewer::Smart {
            return match decision.outcome {
                PermissionApprovalOutcome::AllowOnce
                | PermissionApprovalOutcome::AllowSession
                | PermissionApprovalOutcome::AllowAlways => {
                    self.inner
                        .sandbox_grants
                        .grant_once(tool_call_id, &grant.paths)
                        .map_err(|err| ToolOutput::error(err.to_string()))?;
                    Ok(())
                }
                PermissionApprovalOutcome::Deny => Err(permission_error(
                    "denied",
                    &format!("smart reviewer denied permission: {reason}"),
                    None,
                )),
            };
        }
        match decision.outcome {
            PermissionApprovalOutcome::AllowOnce => {
                self.inner
                    .sandbox_grants
                    .grant_once(tool_call_id, &grant.paths)
                    .map_err(|err| ToolOutput::error(err.to_string()))?;
                Ok(())
            }
            PermissionApprovalOutcome::AllowSession | PermissionApprovalOutcome::AllowAlways => {
                self.inner
                    .sandbox_grants
                    .grant_once(tool_call_id, &grant.paths)
                    .map_err(|err| ToolOutput::error(err.to_string()))?;
                self.inner
                    .sandbox_grants
                    .grant_session(&session_key, &grant.paths)
                    .map_err(|err| ToolOutput::error(err.to_string()))?;
                Ok(())
            }
            PermissionApprovalOutcome::Deny => Err(permission_error(
                "denied",
                &format!("user denied permission; do not retry the same operation: {reason}"),
                None,
            )),
        }
    }

    async fn request_approval_decision(
        &self,
        request: ApprovalDecisionRequest<'_>,
    ) -> std::result::Result<crate::types::PermissionApprovalDecision, ToolOutput> {
        let ApprovalDecisionRequest {
            tool_call_id,
            tool_name,
            args,
            reason,
            matched_rule,
            suggested_rule,
            allow_always,
            abort,
        } = request;
        if self.inner.mode == PermissionMode::DontAsk {
            return Err(permission_error(
                "denied",
                &format!("permission prompt suppressed by dontAsk: {reason}"),
                matched_rule,
            ));
        }
        if matches!(self.inner.config.approval_policy, ApprovalPolicy::Never) {
            return Err(permission_error(
                "denied",
                &format!("approval_policy=never suppressed prompt: {reason}"),
                matched_rule,
            ));
        }
        let reviewer = self.inner.config.approvals_reviewer;
        let handler = match reviewer {
            ApprovalsReviewer::User => self.inner.approval_handler.as_ref(),
            ApprovalsReviewer::Smart => self.inner.smart_approval_handler.as_ref(),
        };
        let Some(handler) = handler else {
            return Err(permission_error(
                "denied",
                &format!("permission reviewer unavailable; failing closed: {reason}"),
                matched_rule,
            ));
        };
        let timeout_secs = handler.timeout_secs();
        let request = PermissionApprovalRequest {
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            summary: action_summary(tool_name, args),
            reason: if self.inner.config.approvals_reviewer == ApprovalsReviewer::Smart {
                match &self.inner.config.auto_review.model {
                    Some(model) => format!("{reason} (smart reviewer configured: {model})"),
                    None => format!("{reason} (smart reviewer configured)"),
                }
            } else {
                reason.to_string()
            },
            matched_rule: matched_rule.map(str::to_string),
            suggested_rule,
            allow_always,
            timeout_secs,
        };
        let mut pending_approval = self.start_pending_approval(&request);
        let decision = match abort {
            Some(mut abort) if timeout_secs == 0 => {
                tokio::select! {
                    biased;
                    _ = abort.wait_for_abort() => return Err(ToolOutput::error("aborted")),
                    decision = handler.request_permission(request) => decision,
                }
            }
            Some(mut abort) => {
                tokio::select! {
                    biased;
                    _ = abort.wait_for_abort() => return Err(ToolOutput::error("aborted")),
                    decision = time::timeout(
                        Duration::from_secs(timeout_secs),
                        handler.request_permission(request),
                    ) => {
                        decision.unwrap_or_else(|_| crate::types::PermissionApprovalDecision::deny())
                    }
                }
            }
            None if timeout_secs == 0 => handler.request_permission(request).await,
            None => time::timeout(
                Duration::from_secs(timeout_secs),
                handler.request_permission(request),
            )
            .await
            .unwrap_or_else(|_| crate::types::PermissionApprovalDecision::deny()),
        };
        pending_approval.finish(decision.outcome);
        Ok(decision)
    }

    fn sandbox_write_grant_request(
        &self,
        action: &PermissionAction,
    ) -> std::result::Result<Option<SandboxWriteGrantRequest>, ToolOutput> {
        let PermissionAction::File {
            paths,
            mutating: true,
            ..
        } = action
        else {
            return Ok(None);
        };

        let mut grant_paths = Vec::new();
        let mut reasons = Vec::new();
        for target in paths {
            match self
                .inner
                .sandbox_policy
                .write_decision(&target.absolute)
                .map_err(|err| ToolOutput::error(err.to_string()))?
            {
                crate::sandbox::SandboxWriteDecision::Allowed => {}
                crate::sandbox::SandboxWriteDecision::Grantable { path, reason } => {
                    grant_paths.push(path);
                    reasons.push(reason);
                }
                crate::sandbox::SandboxWriteDecision::Denied { reason } => {
                    return Err(ToolOutput::error(format!(
                        "denied by sandbox policy: {reason}"
                    )));
                }
            }
        }

        if grant_paths.is_empty() {
            Ok(None)
        } else {
            Ok(Some(SandboxWriteGrantRequest {
                paths: grant_paths,
                reason: reasons.join("; "),
            }))
        }
    }

    pub(crate) fn persist_permission_grants(&self, grants: &[PersistentPermissionGrant]) {
        let fallback_extends = self.local_profile_fallback_extends();
        for grant in grants {
            let result = match grant {
                PersistentPermissionGrant::Filesystem { path, access } => {
                    append_local_filesystem_grant_with_extends(
                        self.inner.project_config_dir.clone(),
                        path,
                        *access,
                        format!("filesystem:{path}"),
                        &fallback_extends,
                    )
                }
                PersistentPermissionGrant::Network { host, access } => {
                    append_local_network_grant_with_extends(
                        self.inner.project_config_dir.clone(),
                        host,
                        *access,
                        format!("network:{host}"),
                        &fallback_extends,
                    )
                }
                PersistentPermissionGrant::Exec { prefix, decision } => {
                    append_local_exec_policy_rule(
                        self.inner.project_config_dir.clone(),
                        prefix,
                        *decision,
                        format!("exec:{}", prefix.join(" ")),
                    )
                }
                PersistentPermissionGrant::Skill { key, access } => {
                    append_local_skill_grant_with_extends(
                        self.inner.project_config_dir.clone(),
                        key,
                        *access,
                        format!("skill:{key}"),
                        &fallback_extends,
                    )
                }
            };
            let _ = result;
        }
    }

    pub(crate) fn local_profile_fallback_extends(&self) -> String {
        if self.inner.config.default_permissions == "local" {
            ":workspace".to_string()
        } else {
            self.inner.config.default_permissions.clone()
        }
    }

    pub(crate) fn remember_session_grant(&self, key: String) {
        if let Ok(mut grants) = self.inner.session_grants.lock() {
            grants.insert(key);
        }
    }

    pub(crate) fn start_pending_approval(
        &self,
        request: &PermissionApprovalRequest,
    ) -> PendingApprovalGuard {
        if let Ok(mut pending) = self.inner.pending_approvals.lock() {
            pending.push_back(request.tool_call_id.clone());
        }
        if let Ok(mut events) = self.inner.approval_events.lock() {
            events.push(ApprovalLifecycleEvent::Requested {
                tool_call_id: request.tool_call_id.clone(),
                tool_name: request.tool_name.clone(),
            });
        }
        PendingApprovalGuard {
            runtime: self.clone(),
            tool_call_id: request.tool_call_id.clone(),
            finished: false,
        }
    }

    pub(crate) fn finish_pending_approval(
        &self,
        tool_call_id: &str,
        outcome: Option<PermissionApprovalOutcome>,
    ) {
        if let Ok(mut pending) = self.inner.pending_approvals.lock()
            && let Some(index) = pending.iter().position(|value| value == tool_call_id)
        {
            pending.remove(index);
        }
        if let Ok(mut events) = self.inner.approval_events.lock() {
            match outcome {
                Some(outcome) => events.push(ApprovalLifecycleEvent::Resolved {
                    tool_call_id: tool_call_id.to_string(),
                    outcome,
                }),
                None => events.push(ApprovalLifecycleEvent::Aborted {
                    tool_call_id: tool_call_id.to_string(),
                }),
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn clear_pending_approval_state(&self) {
        let pending = self
            .inner
            .pending_approvals
            .lock()
            .map(|mut pending| pending.drain(..).collect::<Vec<_>>())
            .unwrap_or_default();
        if let Ok(mut events) = self.inner.approval_events.lock() {
            for tool_call_id in pending {
                events.push(ApprovalLifecycleEvent::Aborted { tool_call_id });
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn approval_lifecycle_events(&self) -> Vec<ApprovalLifecycleEvent> {
        self.inner
            .approval_events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }

    pub(crate) fn has_session_grant(&self, key: &str) -> bool {
        self.inner
            .session_grants
            .lock()
            .is_ok_and(|grants| grants.contains(key))
    }

    pub(crate) fn evaluate(&self, tool_name: &str, args: &Value) -> PermissionDecision {
        let action = PermissionAction::from_tool_call(&self.inner.workdir, tool_name, args);
        let Some(action) = action else {
            return PermissionDecision::Allow;
        };

        if let Some(reason) = hardline_deny(&action) {
            return PermissionDecision::Deny {
                reason,
                matched_rule: None,
            };
        }

        let evaluation = self.evaluate_action_policy(&action);
        if let ActionPolicyEvaluation::Deny {
            reason,
            matched_rule,
        } = evaluation.clone()
        {
            return PermissionDecision::Deny {
                reason,
                matched_rule,
            };
        }

        let session_key = action.session_key();
        if self.has_session_grant(&session_key)
            || self.inner.mode == PermissionMode::BypassPermissions
        {
            return PermissionDecision::Allow;
        }

        self.evaluation_to_permission_decision(&action, session_key, evaluation)
    }

    #[allow(dead_code)]
    pub(crate) fn matching_rule<'a>(
        &'a self,
        rules: &'a [PermissionRule],
        action: &PermissionAction,
    ) -> Option<&'a PermissionRule> {
        rules.iter().find(|rule| action.matches_rule(rule))
    }

    pub(crate) fn evaluation_to_permission_decision(
        &self,
        action: &PermissionAction,
        session_key: String,
        evaluation: ActionPolicyEvaluation,
    ) -> PermissionDecision {
        match evaluation {
            ActionPolicyEvaluation::Allow => PermissionDecision::Allow,
            ActionPolicyEvaluation::Deny {
                reason,
                matched_rule,
            } => PermissionDecision::Deny {
                reason,
                matched_rule,
            },
            ActionPolicyEvaluation::Ask {
                reason,
                matched_rule,
                suggested_rule,
                persistent_grants,
            } => {
                if self.inner.mode == PermissionMode::AcceptEdits
                    && action.is_safe_file_edit()
                    && action.file_targets_all_within_workdir()
                {
                    return PermissionDecision::Allow;
                }
                if matches!(self.inner.config.approval_policy, ApprovalPolicy::Granular)
                    && !self.granular_allows_prompt(action)
                {
                    return PermissionDecision::Deny {
                        reason: format!(
                            "granular approval disabled for {}: {reason}",
                            action.category()
                        ),
                        matched_rule,
                    };
                }
                PermissionDecision::Ask {
                    reason,
                    matched_rule,
                    suggested_rule,
                    allow_always: !persistent_grants.is_empty(),
                    session_key,
                    persistent_grants,
                }
            }
        }
    }

    pub(crate) fn evaluate_action_policy(
        &self,
        action: &PermissionAction,
    ) -> ActionPolicyEvaluation {
        if let Some(decision) = self.exec_policy_decision(action) {
            return decision;
        }
        if let Some(decision) = self.exec_safety_decision(action) {
            return decision;
        }
        let default_permissions = self.inner.config.default_permissions.as_str();
        self.profile_decision(default_permissions, action, &mut BTreeSet::new())
            .unwrap_or_else(|| builtin_profile_decision(":workspace", action))
    }

    pub(crate) fn exec_safety_decision(
        &self,
        action: &PermissionAction,
    ) -> Option<ActionPolicyEvaluation> {
        let PermissionAction::ExecCommand {
            command,
            normalized,
            workdir,
        } = action
        else {
            return None;
        };
        if workdir
            .as_ref()
            .is_some_and(|target| !target.within_workdir)
        {
            return Some(ActionPolicyEvaluation::Ask {
                reason: "command workdir outside accepted workdir requires approval".to_string(),
                matched_rule: None,
                suggested_rule: action.suggested_rule(),
                persistent_grants: action.persistent_grants(),
            });
        }
        if let Some(reason) = dangerous_bash_reason(normalized) {
            return Some(ActionPolicyEvaluation::Ask {
                reason,
                matched_rule: None,
                suggested_rule: action.suggested_rule(),
                persistent_grants: action.persistent_grants(),
            });
        }
        let tokens = command_tokens(command);
        if tokens.is_empty() {
            return None;
        }
        if let Some(inline) = inline_interpreter_review(command, &tokens) {
            return match inline {
                InlineInterpreterReview::LiteralFileReads(paths)
                    if self.file_reads_allowed_by_active_profile(&paths) =>
                {
                    Some(ActionPolicyEvaluation::Allow)
                }
                InlineInterpreterReview::LiteralFileReads(paths) => {
                    Some(ActionPolicyEvaluation::Ask {
                        reason: format!(
                            "inline interpreter reads outside current filesystem permissions: {}",
                            paths.join(", ")
                        ),
                        matched_rule: None,
                        suggested_rule: action.suggested_rule(),
                        persistent_grants: action.persistent_grants(),
                    })
                }
                InlineInterpreterReview::NeedsApproval(reason) => {
                    Some(ActionPolicyEvaluation::Ask {
                        reason,
                        matched_rule: None,
                        suggested_rule: action.suggested_rule(),
                        persistent_grants: action.persistent_grants(),
                    })
                }
            };
        }
        if is_known_safe_command(&tokens) {
            return Some(ActionPolicyEvaluation::Allow);
        }
        if self.active_profile_is_read_only() {
            return Some(ActionPolicyEvaluation::Ask {
                reason: "exec action requires approval under :read-only".to_string(),
                matched_rule: None,
                suggested_rule: action.suggested_rule(),
                persistent_grants: action.persistent_grants(),
            });
        }
        if !self.active_profile_is_configured() {
            return None;
        }
        Some(ActionPolicyEvaluation::Allow)
    }

    pub(crate) fn file_reads_allowed_by_active_profile(&self, paths: &[String]) -> bool {
        if paths.is_empty() {
            return false;
        }
        let targets = paths
            .iter()
            .map(|path| file_target(&self.inner.workdir, path))
            .collect::<Vec<_>>();
        if targets
            .iter()
            .any(|target| protected_read_reason(target).is_some())
        {
            return false;
        }
        let action = PermissionAction::File {
            tool: "read".to_string(),
            paths: targets,
            mutating: false,
        };
        matches!(
            self.profile_decision(
                self.inner.config.default_permissions.as_str(),
                &action,
                &mut BTreeSet::new()
            ),
            Some(ActionPolicyEvaluation::Allow)
        )
    }

    pub(crate) fn active_profile_is_read_only(&self) -> bool {
        self.profile_extends_builtin(self.inner.config.default_permissions.as_str(), ":read-only")
    }

    pub(crate) fn active_profile_is_configured(&self) -> bool {
        let profile = self.inner.config.default_permissions.as_str();
        profile.starts_with(':') || self.inner.config.profiles.contains_key(profile)
    }

    pub(crate) fn profile_extends_builtin(&self, profile_name: &str, builtin: &str) -> bool {
        if profile_name == builtin {
            return true;
        }
        if profile_name.starts_with(':') {
            return false;
        }
        let mut seen = BTreeSet::new();
        let mut current = profile_name;
        while seen.insert(current.to_string()) {
            let Some(profile) = self.inner.config.profiles.get(current) else {
                return false;
            };
            let Some(parent) = profile.extends.as_deref() else {
                return builtin == ":workspace";
            };
            if parent == builtin {
                return true;
            }
            if parent.starts_with(':') {
                return false;
            }
            current = parent;
        }
        false
    }

    pub(crate) fn exec_policy_decision(
        &self,
        action: &PermissionAction,
    ) -> Option<ActionPolicyEvaluation> {
        let PermissionAction::ExecCommand { command, .. } = action else {
            return None;
        };
        let tokens = command_tokens(command);
        if tokens.is_empty() {
            return None;
        }
        let mut prompt: Option<&crate::types::ExecPolicyRule> = None;
        let mut allow: Option<&crate::types::ExecPolicyRule> = None;
        for rule in &self.inner.config.exec_policy.rules {
            if !exec_prefix_matches(
                &rule.prefix,
                &tokens,
                Some(&self.inner.config.exec_policy.host_executables),
            ) {
                continue;
            }
            let prefix = exec_prefix_label(&rule.prefix);
            match rule.decision {
                ExecPolicyDecision::Deny => {
                    return Some(ActionPolicyEvaluation::Deny {
                        reason: rule
                            .justification
                            .clone()
                            .unwrap_or_else(|| format!("blocked by exec_policy prefix `{prefix}`")),
                        matched_rule: Some(format!("exec_policy:{prefix}")),
                    });
                }
                ExecPolicyDecision::Prompt => prompt = Some(rule),
                ExecPolicyDecision::Allow => allow = Some(rule),
            }
        }
        if let Some(rule) = prompt {
            let prefix = exec_prefix_label(&rule.prefix);
            return Some(ActionPolicyEvaluation::Ask {
                reason: rule
                    .justification
                    .clone()
                    .unwrap_or_else(|| format!("exec_policy prefix `{prefix}` requires approval")),
                matched_rule: Some(format!("exec_policy:{prefix}")),
                suggested_rule: Some(format!("exec:{}", tokens.join(" "))),
                persistent_grants: action.persistent_grants(),
            });
        }
        allow.map(|_| ActionPolicyEvaluation::Allow)
    }

    pub(crate) fn profile_decision(
        &self,
        profile_name: &str,
        action: &PermissionAction,
        seen: &mut BTreeSet<String>,
    ) -> Option<ActionPolicyEvaluation> {
        if profile_name.starts_with(':') {
            return Some(builtin_profile_decision(profile_name, action));
        }
        if !seen.insert(profile_name.to_string()) {
            return Some(ActionPolicyEvaluation::Deny {
                reason: format!("permission profile `{profile_name}` extends cycle detected"),
                matched_rule: Some(format!("permissions.{profile_name}")),
            });
        }
        let Some(profile) = self.inner.config.profiles.get(profile_name) else {
            return Some(ActionPolicyEvaluation::Deny {
                reason: format!("permission profile `{profile_name}` is not configured"),
                matched_rule: Some(format!("permissions.{profile_name}")),
            });
        };
        if let Some(decision) = explicit_profile_decision(profile_name, profile, action) {
            return Some(decision);
        }
        match profile.extends.as_deref() {
            Some(parent) => self.profile_decision(parent, action, seen),
            None => Some(builtin_profile_decision(":workspace", action)),
        }
    }

    pub(crate) fn granular_allows_prompt(&self, action: &PermissionAction) -> bool {
        let Some(granular) = &self.inner.config.granular else {
            return false;
        };
        match action {
            PermissionAction::ExecCommand { .. } => granular.exec,
            PermissionAction::File { .. } => granular.filesystem,
            PermissionAction::Skill { .. } => granular.skill,
            PermissionAction::McpStartup { .. } | PermissionAction::Mcp { .. } => granular.mcp,
            PermissionAction::WebFetch { .. } => granular.network,
        }
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PermissionAction {
    ExecCommand {
        command: String,
        normalized: String,
        workdir: Option<FileTarget>,
    },
    File {
        tool: String,
        paths: Vec<FileTarget>,
        mutating: bool,
    },
    Skill {
        tool: String,
        action: String,
    },
    McpStartup {
        server: String,
        transport: String,
    },
    Mcp {
        server: String,
        tool: String,
    },
    WebFetch {
        url: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileTarget {
    pub(crate) raw: String,
    pub(crate) absolute: PathBuf,
    pub(crate) relative: String,
    pub(crate) within_workdir: bool,
}

impl PermissionAction {
    pub(crate) fn from_tool_call(workdir: &Path, tool_name: &str, args: &Value) -> Option<Self> {
        match tool_name {
            "exec_command" => {
                args.get("cmd")
                    .and_then(Value::as_str)
                    .map(|command| Self::ExecCommand {
                        command: command.to_string(),
                        normalized: normalize_command(command),
                        workdir: args
                            .get("workdir")
                            .and_then(Value::as_str)
                            .map(|path| file_target(workdir, path)),
                    })
            }
            "read" => file_paths_from_args(workdir, args, &["path"]).map(|paths| Self::File {
                tool: "read".to_string(),
                paths,
                mutating: false,
            }),
            "write" => file_paths_from_args(workdir, args, &["path"]).map(|paths| Self::File {
                tool: "write".to_string(),
                paths,
                mutating: true,
            }),
            "edit" => {
                let paths = edit_paths_from_args(workdir, args);
                (!paths.is_empty()).then(|| Self::File {
                    tool: "edit".to_string(),
                    paths,
                    mutating: true,
                })
            }
            "skill_manage" => {
                args.get("action")
                    .and_then(Value::as_str)
                    .map(|action| Self::Skill {
                        tool: "skill_manage".to_string(),
                        action: action.to_string(),
                    })
            }
            "skill_hub" => args
                .get("action")
                .and_then(Value::as_str)
                .and_then(|action| {
                    (!matches!(
                        action,
                        "browse" | "search" | "inspect" | "list" | "check" | "audit"
                    ))
                    .then(|| Self::Skill {
                        tool: "skill_hub".to_string(),
                        action: action.to_string(),
                    })
                }),
            "skill_config" => args
                .get("action")
                .and_then(Value::as_str)
                .and_then(|action| {
                    (action != "status").then(|| Self::Skill {
                        tool: "skill_config".to_string(),
                        action: action.to_string(),
                    })
                }),
            "web_fetch" => args
                .get("url")
                .and_then(Value::as_str)
                .map(|url| Self::WebFetch {
                    url: url.to_string(),
                }),
            "mcp_startup" => {
                args.get("server")
                    .and_then(Value::as_str)
                    .map(|server| Self::McpStartup {
                        server: server.to_string(),
                        transport: args
                            .get("transport")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown")
                            .to_string(),
                    })
            }
            _ => crate::mcp::mcp_tool_name_parts(tool_name).map(|(server, tool)| Self::Mcp {
                server: server.to_string(),
                tool: tool.to_string(),
            }),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn matches_rule(&self, rule: &PermissionRule) -> bool {
        match self {
            Self::ExecCommand { normalized, .. } => {
                rule.tool == "exec_command" && wildcard_match(&rule.pattern, normalized)
            }
            Self::File { tool, paths, .. } => {
                rule.tool == *tool
                    && paths.iter().any(|target| {
                        if Path::new(&rule.pattern).is_absolute() {
                            wildcard_match(&rule.pattern, &target.absolute.to_string_lossy())
                        } else {
                            wildcard_match(&rule.pattern, &target.relative)
                        }
                    })
            }
            Self::Skill { tool, action } => {
                rule.tool == *tool && wildcard_match(&rule.pattern, action)
            }
            Self::McpStartup { server, .. } => {
                rule.tool == "mcp_startup" && wildcard_match(&rule.pattern, server)
            }
            Self::Mcp { server, tool } => {
                rule.tool == "mcp" && wildcard_match(&rule.pattern, &format!("{server}/{tool}"))
            }
            Self::WebFetch { url } => {
                rule.tool == "web_fetch" && wildcard_match(&rule.pattern, url)
            }
        }
    }

    pub(crate) fn session_key(&self) -> String {
        match self {
            Self::ExecCommand {
                command,
                normalized,
                ..
            } => exec_grant_prefix(command)
                .map(|prefix| format!("exec_policy:{}", prefix.join(" ")))
                .unwrap_or_else(|| format!("exec_command:{normalized}")),
            Self::File { tool, paths, .. } => format!(
                "{tool}:{}",
                paths
                    .iter()
                    .map(|target| target.relative.clone())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Self::Skill { tool, action } => format!("{tool}:{action}"),
            Self::McpStartup { server, .. } => format!("mcp_startup:{server}"),
            Self::Mcp { server, tool } => format!("mcp:{server}/{tool}"),
            Self::WebFetch { url } => format!("web_fetch:{url}"),
        }
    }

    pub(crate) fn suggested_rule(&self) -> Option<String> {
        match self {
            Self::ExecCommand { command, .. } => exec_grant_prefix(command)
                .map(|prefix| format!("exec:{}", prefix.join(" ")))
                .or_else(|| Some(format!("ExecCommand({command})"))),
            Self::File { .. } => None,
            Self::Skill { tool, action } => {
                Some(format!("{}({action})", permission_rule_tool(tool)))
            }
            Self::McpStartup { server, .. } => Some(format!("McpStartup({server})")),
            Self::Mcp { server, tool } => Some(format!("Mcp({server}/{tool})")),
            Self::WebFetch { url } => Some(format!("WebFetch({url})")),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn allow_always(&self) -> bool {
        matches!(
            self,
            Self::ExecCommand { .. }
                | Self::Skill { .. }
                | Self::McpStartup { .. }
                | Self::Mcp { .. }
                | Self::WebFetch { .. }
        )
    }

    pub(crate) fn persistent_grants(&self) -> Vec<PersistentPermissionGrant> {
        match self {
            Self::ExecCommand { command, .. } => {
                let prefix = exec_grant_prefix(command).unwrap_or_else(|| command_tokens(command));
                (!prefix.is_empty())
                    .then_some(PersistentPermissionGrant::Exec {
                        prefix,
                        decision: ExecPolicyDecision::Allow,
                    })
                    .into_iter()
                    .collect()
            }
            Self::File {
                paths, mutating, ..
            } => {
                let access = if *mutating {
                    PermissionAccess::Write
                } else {
                    PermissionAccess::Read
                };
                paths
                    .iter()
                    .map(|target| PersistentPermissionGrant::Filesystem {
                        path: target.absolute.to_string_lossy().to_string(),
                        access,
                    })
                    .collect()
            }
            Self::Skill { tool, action } => vec![PersistentPermissionGrant::Skill {
                key: format!("{tool}/{action}"),
                access: PermissionAccess::Allow,
            }],
            Self::WebFetch { url } => web_fetch_host(url)
                .map(|host| {
                    vec![PersistentPermissionGrant::Network {
                        host,
                        access: PermissionAccess::Allow,
                    }]
                })
                .unwrap_or_default(),
            Self::McpStartup { .. } | Self::Mcp { .. } => Vec::new(),
        }
    }

    pub(crate) fn file_targets_all_within_workdir(&self) -> bool {
        match self {
            Self::File { paths, .. } => paths.iter().all(|path| path.within_workdir),
            _ => false,
        }
    }

    pub(crate) fn category(&self) -> &'static str {
        match self {
            Self::ExecCommand { .. } => "exec",
            Self::File { .. } => "filesystem",
            Self::Skill { .. } => "skill",
            Self::McpStartup { .. } | Self::Mcp { .. } => "mcp",
            Self::WebFetch { .. } => "network",
        }
    }

    pub(crate) fn is_safe_file_edit(&self) -> bool {
        matches!(
            self,
            Self::File {
                mutating: true,
                paths,
                ..
            } if paths.iter().all(|path| protected_write_reason(path).is_none())
        )
    }
}

pub(crate) fn file_paths_from_args(
    workdir: &Path,
    args: &Value,
    keys: &[&str],
) -> Option<Vec<FileTarget>> {
    let paths = keys
        .iter()
        .filter_map(|key| args.get(*key).and_then(Value::as_str))
        .map(|path| file_target(workdir, path))
        .collect::<Vec<_>>();
    (!paths.is_empty()).then_some(paths)
}

pub(crate) fn edit_paths_from_args(workdir: &Path, args: &Value) -> Vec<FileTarget> {
    if let Some(paths) = file_paths_from_args(workdir, args, &["path"]) {
        return paths;
    }
    args.get("patch")
        .and_then(Value::as_str)
        .map(|patch| {
            patch
                .lines()
                .flat_map(patch_file_paths)
                .map(|path| file_target(workdir, &path))
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn patch_file_paths(line: &str) -> Vec<String> {
    let line = line.trim();
    for marker in [
        "*** Update File:",
        "*** Add File:",
        "*** Delete File:",
        "*** Move to:",
    ] {
        if let Some(path) = line.strip_prefix(marker) {
            return vec![path.trim().to_string()];
        }
    }
    if let Some(rest) = line.strip_prefix("*** Move File:") {
        if let Some((from, to)) = rest.split_once("->") {
            return vec![from.trim().to_string(), to.trim().to_string()];
        }
        return vec![rest.trim().to_string()];
    }
    Vec::new()
}

pub(crate) fn file_target(workdir: &Path, raw: &str) -> FileTarget {
    let path = Path::new(raw);
    let absolute = if path.is_absolute() {
        lexical_normalize(path)
    } else {
        lexical_normalize(&workdir.join(path))
    };
    let (relative, within_workdir) = absolute
        .strip_prefix(workdir)
        .map(|path| (path.to_string_lossy().replace('\\', "/"), true))
        .unwrap_or_else(|_| (raw.replace('\\', "/"), false));
    FileTarget {
        raw: raw.to_string(),
        absolute,
        relative,
        within_workdir,
    }
}

pub(crate) fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

pub(crate) fn hardline_deny(action: &PermissionAction) -> Option<String> {
    match action {
        PermissionAction::ExecCommand { normalized, .. } => {
            background_shell_reason(normalized).or_else(|| hardline_bash_reason(normalized))
        }
        PermissionAction::File {
            paths, mutating, ..
        } => paths.iter().find_map(|target| {
            if *mutating {
                protected_write_reason(target)
            } else {
                protected_read_reason(target)
            }
        }),
        PermissionAction::Skill { .. } => None,
        PermissionAction::McpStartup { .. } => None,
        PermissionAction::Mcp { .. } => None,
        PermissionAction::WebFetch { .. } => None,
    }
}

pub(crate) fn default_ask_reason(action: &PermissionAction) -> Option<String> {
    match action {
        PermissionAction::ExecCommand {
            normalized,
            workdir,
            ..
        } => {
            if workdir
                .as_ref()
                .is_some_and(|target| !target.within_workdir)
            {
                return Some(
                    "command workdir outside accepted workdir requires approval".to_string(),
                );
            }
            dangerous_bash_reason(normalized)
        }
        PermissionAction::File { .. } => None,
        PermissionAction::Skill { tool, action } => Some(format!(
            "{tool} action `{action}` changes skill configuration or files and requires approval"
        )),
        PermissionAction::McpStartup { server, transport } => Some(format!(
            "MCP server `{server}` startup over {transport} requires approval"
        )),
        PermissionAction::Mcp { server, tool } => {
            Some(format!("MCP tool `{server}/{tool}` requires approval"))
        }
        PermissionAction::WebFetch { .. } => None,
    }
}

pub(crate) fn builtin_profile_decision(
    profile_name: &str,
    action: &PermissionAction,
) -> ActionPolicyEvaluation {
    match profile_name {
        ":danger-full-access" => ActionPolicyEvaluation::Allow,
        ":read-only" => read_only_profile_decision(action),
        ":workspace" => workspace_profile_decision(action),
        other => ActionPolicyEvaluation::Deny {
            reason: format!("unknown built-in permission profile `{other}`"),
            matched_rule: Some(other.to_string()),
        },
    }
}

pub(crate) fn workspace_profile_decision(action: &PermissionAction) -> ActionPolicyEvaluation {
    match action {
        PermissionAction::File {
            paths, mutating, ..
        } => {
            if paths.iter().all(|target| target.within_workdir) {
                return ActionPolicyEvaluation::Allow;
            }
            let outside = paths
                .iter()
                .filter(|target| !target.within_workdir)
                .map(|target| target.absolute.to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            ActionPolicyEvaluation::Ask {
                reason: format!(
                    "{} outside workdir requires approval: {outside}",
                    if *mutating { "file write" } else { "file read" }
                ),
                matched_rule: None,
                suggested_rule: Some(format!("filesystem:{outside}")),
                persistent_grants: action.persistent_grants(),
            }
        }
        PermissionAction::ExecCommand { .. } => {
            if let Some(reason) = default_ask_reason(action) {
                ActionPolicyEvaluation::Ask {
                    reason,
                    matched_rule: None,
                    suggested_rule: action.suggested_rule(),
                    persistent_grants: action.persistent_grants(),
                }
            } else {
                ActionPolicyEvaluation::Allow
            }
        }
        PermissionAction::Skill {
            tool,
            action: skill_action,
        } => ActionPolicyEvaluation::Ask {
            reason: format!(
                "{tool} action `{skill_action}` changes skill configuration or files and requires approval"
            ),
            matched_rule: None,
            suggested_rule: Some(format!("skill:{tool}/{skill_action}")),
            persistent_grants: action.persistent_grants(),
        },
        PermissionAction::McpStartup { server, transport } => ActionPolicyEvaluation::Ask {
            reason: format!("MCP server `{server}` startup over {transport} requires approval"),
            matched_rule: None,
            suggested_rule: Some(format!("mcp_startup:{server}")),
            persistent_grants: action.persistent_grants(),
        },
        PermissionAction::Mcp { server, tool } => ActionPolicyEvaluation::Ask {
            reason: format!("MCP tool `{server}/{tool}` requires approval"),
            matched_rule: None,
            suggested_rule: Some(format!("mcp:{server}/{tool}")),
            persistent_grants: action.persistent_grants(),
        },
        PermissionAction::WebFetch { .. } => ActionPolicyEvaluation::Allow,
    }
}

pub(crate) fn read_only_profile_decision(action: &PermissionAction) -> ActionPolicyEvaluation {
    match action {
        PermissionAction::File {
            paths,
            mutating: false,
            ..
        } if paths.iter().all(|target| target.within_workdir) => ActionPolicyEvaluation::Allow,
        PermissionAction::File { .. } => ActionPolicyEvaluation::Ask {
            reason:
                "read-only permissions require approval for file writes or outside-workdir reads"
                    .to_string(),
            matched_rule: None,
            suggested_rule: action.suggested_rule(),
            persistent_grants: action.persistent_grants(),
        },
        _ => ActionPolicyEvaluation::Ask {
            reason: format!(
                "{} action requires approval under :read-only",
                action.category()
            ),
            matched_rule: None,
            suggested_rule: action.suggested_rule(),
            persistent_grants: action.persistent_grants(),
        },
    }
}

pub(crate) fn explicit_profile_decision(
    profile_name: &str,
    profile: &PermissionProfileConfig,
    action: &PermissionAction,
) -> Option<ActionPolicyEvaluation> {
    match action {
        PermissionAction::File {
            paths, mutating, ..
        } => profile_filesystem_decision(profile_name, &profile.filesystem, paths, *mutating),
        PermissionAction::WebFetch { url } => web_fetch_host(url).and_then(|host| {
            profile_access_decision(
                profile_name,
                "network",
                &profile.network_domains,
                &host,
                || format!("network access to `{host}` requires approval"),
                || action.persistent_grants(),
            )
        }),
        PermissionAction::Skill { tool, action } => {
            let key = format!("{tool}/{action}");
            profile_access_decision(
                profile_name,
                "skill",
                &profile.skill_tools,
                &key,
                || format!("{tool} action `{action}` requires approval"),
                || {
                    vec![PersistentPermissionGrant::Skill {
                        key: key.clone(),
                        access: PermissionAccess::Allow,
                    }]
                },
            )
        }
        PermissionAction::ExecCommand { .. }
        | PermissionAction::McpStartup { .. }
        | PermissionAction::Mcp { .. } => None,
    }
}

pub(crate) fn profile_filesystem_decision(
    profile_name: &str,
    rules: &std::collections::BTreeMap<String, PermissionAccess>,
    paths: &[FileTarget],
    mutating: bool,
) -> Option<ActionPolicyEvaluation> {
    let mut matched_allow = 0usize;
    for target in paths {
        let Some((rule, access)) = matching_filesystem_access(rules, target) else {
            continue;
        };
        match access {
            PermissionAccess::Deny => {
                return Some(ActionPolicyEvaluation::Deny {
                    reason: format!("blocked by permissions.{profile_name}.filesystem `{rule}`"),
                    matched_rule: Some(format!("permissions.{profile_name}.filesystem.{rule}")),
                });
            }
            PermissionAccess::Prompt => {
                return Some(ActionPolicyEvaluation::Ask {
                    reason: format!(
                        "permissions.{profile_name}.filesystem `{rule}` requires approval"
                    ),
                    matched_rule: Some(format!("permissions.{profile_name}.filesystem.{rule}")),
                    suggested_rule: Some(format!(
                        "filesystem:{}",
                        target.absolute.to_string_lossy()
                    )),
                    persistent_grants: vec![PersistentPermissionGrant::Filesystem {
                        path: target.absolute.to_string_lossy().to_string(),
                        access: if mutating {
                            PermissionAccess::Write
                        } else {
                            PermissionAccess::Read
                        },
                    }],
                });
            }
            PermissionAccess::Read if mutating => {
                return Some(ActionPolicyEvaluation::Ask {
                    reason: format!(
                        "permissions.{profile_name}.filesystem `{rule}` allows read only"
                    ),
                    matched_rule: Some(format!("permissions.{profile_name}.filesystem.{rule}")),
                    suggested_rule: Some(format!(
                        "filesystem:{}",
                        target.absolute.to_string_lossy()
                    )),
                    persistent_grants: vec![PersistentPermissionGrant::Filesystem {
                        path: target.absolute.to_string_lossy().to_string(),
                        access: PermissionAccess::Write,
                    }],
                });
            }
            PermissionAccess::Read | PermissionAccess::Write | PermissionAccess::Allow => {
                matched_allow += 1;
            }
        }
    }
    (matched_allow == paths.len() && !paths.is_empty()).then_some(ActionPolicyEvaluation::Allow)
}

pub(crate) fn profile_access_decision<F, G>(
    profile_name: &str,
    category: &str,
    rules: &std::collections::BTreeMap<String, PermissionAccess>,
    target: &str,
    prompt_reason: F,
    persistent_grants: G,
) -> Option<ActionPolicyEvaluation>
where
    F: FnOnce() -> String,
    G: FnOnce() -> Vec<PersistentPermissionGrant>,
{
    let (rule, access) = matching_access(rules, target)?;
    match access {
        PermissionAccess::Deny => Some(ActionPolicyEvaluation::Deny {
            reason: format!("blocked by permissions.{profile_name}.{category} `{rule}`"),
            matched_rule: Some(format!("permissions.{profile_name}.{category}.{rule}")),
        }),
        PermissionAccess::Prompt => Some(ActionPolicyEvaluation::Ask {
            reason: prompt_reason(),
            matched_rule: Some(format!("permissions.{profile_name}.{category}.{rule}")),
            suggested_rule: Some(format!("{category}:{target}")),
            persistent_grants: persistent_grants(),
        }),
        PermissionAccess::Read | PermissionAccess::Write | PermissionAccess::Allow => {
            Some(ActionPolicyEvaluation::Allow)
        }
    }
}

pub(crate) fn matching_filesystem_access<'a>(
    rules: &'a std::collections::BTreeMap<String, PermissionAccess>,
    target: &FileTarget,
) -> Option<(&'a str, PermissionAccess)> {
    rules
        .iter()
        .filter(|(rule, _)| filesystem_rule_matches(rule, target))
        .max_by_key(|(rule, _)| rule.len())
        .map(|(rule, access)| (rule.as_str(), *access))
}

pub(crate) fn filesystem_rule_matches(rule: &str, target: &FileTarget) -> bool {
    let rule_path = Path::new(rule);
    if rule_path.is_absolute() {
        let normalized = lexical_normalize(rule_path);
        return target.absolute == normalized || target.absolute.starts_with(&normalized);
    }
    let rule = rule.replace('\\', "/");
    target.relative == rule
        || target
            .relative
            .strip_prefix(&rule)
            .is_some_and(|rest| rest.starts_with('/'))
}

pub(crate) fn matching_access<'a>(
    rules: &'a std::collections::BTreeMap<String, PermissionAccess>,
    target: &str,
) -> Option<(&'a str, PermissionAccess)> {
    rules
        .iter()
        .filter(|(rule, _)| access_rule_matches(rule, target))
        .max_by_key(|(rule, _)| rule.len())
        .map(|(rule, access)| (rule.as_str(), *access))
}

pub(crate) fn access_rule_matches(rule: &str, target: &str) -> bool {
    let rule = rule.to_ascii_lowercase();
    let target = target.to_ascii_lowercase();
    if rule == target || wildcard_match(&rule, &target) {
        return true;
    }
    target
        .strip_suffix(&rule)
        .is_some_and(|prefix| prefix.ends_with('.'))
}

pub(crate) fn web_fetch_host(value: &str) -> Option<String> {
    let rest = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"))
        .unwrap_or(value);
    rest.split('/')
        .next()
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .map(str::to_ascii_lowercase)
}

pub(crate) fn command_tokens(command: &str) -> Vec<String> {
    shell_command_tokens(command).unwrap_or_default()
}

pub(crate) fn exec_prefix_matches(
    prefix: &[ExecPolicyPatternToken],
    tokens: &[String],
    host_executables: Option<&[crate::types::ExecPolicyHostExecutable]>,
) -> bool {
    if prefix.len() > tokens.len() || prefix.is_empty() {
        return false;
    }
    if prefix
        .iter()
        .zip(tokens)
        .all(|(pattern, token)| pattern.matches(token))
    {
        return true;
    }
    let Some(host_executables) = host_executables else {
        return false;
    };
    let Some(first_token) = tokens.first() else {
        return false;
    };
    if !Path::new(first_token).is_absolute() {
        return false;
    }
    let Some(basename) = shell_basename(first_token) else {
        return false;
    };
    let first_pattern = &prefix[0];
    if !first_pattern.matches(&basename) {
        return false;
    }
    if !host_executable_allows_path(host_executables, &basename, first_token) {
        return false;
    }
    prefix
        .iter()
        .skip(1)
        .zip(tokens.iter().skip(1))
        .all(|(pattern, token)| pattern.matches(token))
}

pub(crate) fn exec_prefix_label(prefix: &[ExecPolicyPatternToken]) -> String {
    prefix
        .iter()
        .map(|token| match token {
            ExecPolicyPatternToken::Single(value) => value.clone(),
            ExecPolicyPatternToken::Alternatives(values) => format!("[{}]", values.join("|")),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn host_executable_allows_path(
    host_executables: &[crate::types::ExecPolicyHostExecutable],
    name: &str,
    path: &str,
) -> bool {
    match host_executables.iter().find(|host| host.name == name) {
        Some(host) => host.paths.iter().any(|allowed| allowed == path),
        None => true,
    }
}

pub(crate) fn exec_grant_prefix(command: &str) -> Option<Vec<String>> {
    let direct = command_tokens(command);
    if direct.is_empty() {
        return None;
    }
    if let Some(commands) = shell_lc_word_only_commands(&direct) {
        return commands
            .into_iter()
            .find(|command| !is_known_safe_command(command))
            .and_then(|command| risky_command_prefix(&command));
    }
    risky_command_prefix(&direct)
}

pub(crate) fn risky_command_prefix(command: &[String]) -> Option<Vec<String>> {
    if command.is_empty() {
        return None;
    }
    if is_inline_interpreter_tokens(command) {
        return Some(command.iter().take(2).cloned().collect());
    }
    if shell_basename(&command[0]).as_deref() == Some("git")
        && let Some((_index, subcommand)) = git_subcommand(command)
    {
        return Some(vec![command[0].clone(), subcommand.to_string()]);
    }
    Some(command.iter().take(command.len().min(2)).cloned().collect())
}

pub(crate) fn permission_rule_tool(tool: &str) -> &str {
    match tool {
        "skill_manage" => "SkillManage",
        "skill_hub" => "SkillHub",
        "skill_config" => "SkillConfig",
        "mcp_startup" => "McpStartup",
        "mcp" => "Mcp",
        "web_fetch" => "WebFetch",
        other => other,
    }
}

pub(crate) fn protected_write_reason(target: &FileTarget) -> Option<String> {
    let rel = target.relative.as_str();
    let rel_lower = rel.to_ascii_lowercase();
    let file_name = Path::new(rel)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if rel == ".psychevo/config.toml" {
        return Some("permission configuration cannot be modified by model tools".to_string());
    }
    if file_name == ".env" {
        return Some("protected credential file write denied".to_string());
    }
    let protected_files = [
        ".bashrc",
        ".zshrc",
        ".profile",
        ".bash_profile",
        ".zprofile",
        ".netrc",
        ".pgpass",
        ".npmrc",
        ".pypirc",
    ];
    if protected_files.contains(&file_name) {
        return Some(format!("protected file write denied: {file_name}"));
    }
    let protected_dirs = [
        ".ssh/",
        ".aws/",
        ".gnupg/",
        ".kube/",
        ".docker/",
        ".azure/",
        ".config/gh/",
    ];
    if protected_dirs
        .iter()
        .any(|prefix| rel_lower == prefix.trim_end_matches('/') || rel_lower.starts_with(prefix))
    {
        return Some("protected credential directory write denied".to_string());
    }
    None
}

pub(crate) fn protected_read_reason(target: &FileTarget) -> Option<String> {
    let rel = target.relative.to_ascii_lowercase();
    if rel.starts_with(".psychevo/skills/.hub/") || rel.starts_with(".psychevo/cache/") {
        return Some("internal Psychevo cache files cannot be read directly".to_string());
    }
    None
}

#[cfg(test)]
mod sandbox_approval_tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Debug)]
    struct RecordingApprovalHandler {
        decisions: Mutex<VecDeque<crate::types::PermissionApprovalDecision>>,
        requests: Mutex<Vec<PermissionApprovalRequest>>,
    }

    impl RecordingApprovalHandler {
        fn new(decisions: Vec<crate::types::PermissionApprovalDecision>) -> Arc<Self> {
            Arc::new(Self {
                decisions: Mutex::new(decisions.into()),
                requests: Mutex::new(Vec::new()),
            })
        }

        fn requests(&self) -> Vec<PermissionApprovalRequest> {
            self.requests.lock().expect("requests").clone()
        }
    }

    impl crate::types::ApprovalHandler for RecordingApprovalHandler {
        fn timeout_secs(&self) -> u64 {
            0
        }

        fn request_permission(
            &self,
            request: PermissionApprovalRequest,
        ) -> BoxFuture<'static, crate::types::PermissionApprovalDecision> {
            self.requests.lock().expect("requests").push(request);
            let decision = self
                .decisions
                .lock()
                .expect("decisions")
                .pop_front()
                .unwrap_or_else(crate::types::PermissionApprovalDecision::deny);
            Box::pin(async move { decision })
        }
    }

    fn abort_signal() -> AbortSignal {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        AbortSignal::new(rx)
    }

    fn sandbox_policy(
        workdir: &Path,
        mode: crate::sandbox::SandboxMode,
    ) -> crate::sandbox::SandboxPolicy {
        crate::sandbox::SandboxPolicy::from_config(
            &crate::sandbox::SandboxConfig {
                enabled: true,
                mode,
                writable_roots: Vec::new(),
                include_tmp: false,
                include_common_caches: false,
            },
            workdir,
            crate::types::RunMode::Default,
            &BTreeMap::new(),
        )
        .expect("sandbox policy")
    }

    fn tool_context(
        policy: crate::sandbox::SandboxPolicy,
        grants: crate::sandbox::SandboxWriteGrants,
    ) -> crate::tools::ToolRuntimeContext {
        crate::tools::ToolRuntimeContext {
            task_id: "sandbox-approval-test".to_string(),
            lsp: crate::config::LspConfig {
                enabled: false,
                ..Default::default()
            },
            lsp_manager: crate::tools::write_support::default_lsp_manager(),
            allow_login_shell: false,
            stream_events: None,
            env: BTreeMap::new(),
            path_prefixes: Vec::new(),
            sandbox_policy: policy,
            sandbox_grants: grants,
        }
    }

    fn permission_runtime(
        workdir: &Path,
        policy: crate::sandbox::SandboxPolicy,
        grants: crate::sandbox::SandboxWriteGrants,
        handler: Arc<RecordingApprovalHandler>,
    ) -> PermissionRuntime {
        permission_runtime_with_config(
            workdir,
            PermissionConfig::default(),
            policy,
            grants,
            handler,
        )
    }

    fn permission_runtime_with_config(
        workdir: &Path,
        config: PermissionConfig,
        policy: crate::sandbox::SandboxPolicy,
        grants: crate::sandbox::SandboxWriteGrants,
        handler: Arc<RecordingApprovalHandler>,
    ) -> PermissionRuntime {
        PermissionRuntime::new(
            workdir.to_path_buf(),
            workdir.join(".psychevo"),
            config,
            PermissionMode::Default,
            ApprovalMode::Manual,
            Some(handler),
            None,
        )
        .with_sandbox(policy, grants)
    }

    fn wrapped_write(
        workdir: &Path,
        policy: crate::sandbox::SandboxPolicy,
        grants: crate::sandbox::SandboxWriteGrants,
        runtime: &PermissionRuntime,
    ) -> Arc<dyn ToolBinding> {
        runtime
            .wrap_tools(vec![Arc::new(crate::tools::WriteTool::new(
                workdir.to_path_buf(),
                tool_context(policy, grants),
            )) as Arc<dyn ToolBinding>])
            .into_iter()
            .next()
            .expect("write tool")
    }

    fn wrapped_edit(
        workdir: &Path,
        policy: crate::sandbox::SandboxPolicy,
        grants: crate::sandbox::SandboxWriteGrants,
        runtime: &PermissionRuntime,
    ) -> Arc<dyn ToolBinding> {
        runtime
            .wrap_tools(vec![Arc::new(crate::tools::EditTool::new(
                workdir.to_path_buf(),
                tool_context(policy, grants),
            )) as Arc<dyn ToolBinding>])
            .into_iter()
            .next()
            .expect("edit tool")
    }

    #[tokio::test]
    async fn allow_once_grants_current_write_call_only() {
        let work = tempfile::tempdir().expect("work");
        let outside = tempfile::tempdir().expect("outside");
        let target = outside.path().join("writer.txt");
        let policy = sandbox_policy(work.path(), crate::sandbox::SandboxMode::WorkspaceWrite);
        let grants = crate::sandbox::SandboxWriteGrants::default();
        let handler = RecordingApprovalHandler::new(vec![
            crate::types::PermissionApprovalDecision::allow_once(),
        ]);
        let runtime =
            permission_runtime(work.path(), policy.clone(), grants.clone(), handler.clone());
        let write = wrapped_write(work.path(), policy, grants, &runtime);

        let first = write
            .execute(
                "call-write-once".to_string(),
                json!({"path": target.display().to_string(), "content": "one\n"}),
                abort_signal(),
            )
            .await;
        assert!(!first.is_error, "{:?}", first.json);
        assert_eq!(std::fs::read_to_string(&target).expect("target"), "one\n");

        let requests = handler.requests();
        assert_eq!(requests.len(), 1);
        assert!(!requests[0].allow_always);
        assert!(requests[0].reason.contains("sandbox approval required"));

        let second = write
            .execute(
                "call-write-second".to_string(),
                json!({"path": target.display().to_string(), "content": "two\n"}),
                abort_signal(),
            )
            .await;
        assert!(second.is_error, "{:?}", second.json);
        assert_eq!(std::fs::read_to_string(&target).expect("target"), "one\n");
    }

    #[tokio::test]
    async fn allow_session_reuses_same_file_key_but_not_sibling() {
        let work = tempfile::tempdir().expect("work");
        let outside = tempfile::tempdir().expect("outside");
        let target = outside.path().join("session.txt");
        let sibling = outside.path().join("sibling.txt");
        let policy = sandbox_policy(work.path(), crate::sandbox::SandboxMode::WorkspaceWrite);
        let grants = crate::sandbox::SandboxWriteGrants::default();
        let handler = RecordingApprovalHandler::new(vec![
            crate::types::PermissionApprovalDecision::allow_session(),
        ]);
        let runtime =
            permission_runtime(work.path(), policy.clone(), grants.clone(), handler.clone());
        let write = wrapped_write(work.path(), policy, grants, &runtime);

        let first = write
            .execute(
                "call-session-1".to_string(),
                json!({"path": target.display().to_string(), "content": "one\n"}),
                abort_signal(),
            )
            .await;
        assert!(!first.is_error, "{:?}", first.json);
        let second = write
            .execute(
                "call-session-2".to_string(),
                json!({"path": target.display().to_string(), "content": "two\n"}),
                abort_signal(),
            )
            .await;
        assert!(!second.is_error, "{:?}", second.json);
        assert_eq!(std::fs::read_to_string(&target).expect("target"), "two\n");
        assert_eq!(handler.requests().len(), 1);

        let sibling_result = write
            .execute(
                "call-session-sibling".to_string(),
                json!({"path": sibling.display().to_string(), "content": "bad\n"}),
                abort_signal(),
            )
            .await;
        assert!(sibling_result.is_error, "{:?}", sibling_result.json);
        assert!(!sibling.exists());
        assert_eq!(handler.requests().len(), 2);
    }

    #[tokio::test]
    async fn read_only_sandbox_denies_without_prompt() {
        let work = tempfile::tempdir().expect("work");
        let target = work.path().join("blocked.txt");
        let policy = sandbox_policy(work.path(), crate::sandbox::SandboxMode::ReadOnly);
        let grants = crate::sandbox::SandboxWriteGrants::default();
        let handler = RecordingApprovalHandler::new(vec![
            crate::types::PermissionApprovalDecision::allow_once(),
        ]);
        let runtime =
            permission_runtime(work.path(), policy.clone(), grants.clone(), handler.clone());
        let write = wrapped_write(work.path(), policy, grants, &runtime);

        let output = write
            .execute(
                "call-read-only".to_string(),
                json!({"path": target.display().to_string(), "content": "no\n"}),
                abort_signal(),
            )
            .await;

        assert!(output.is_error, "{:?}", output.json);
        assert!(
            output.json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("read-only")
        );
        assert!(!target.exists());
        assert!(handler.requests().is_empty());
    }

    #[tokio::test]
    async fn allow_once_grants_current_edit_call() {
        let work = tempfile::tempdir().expect("work");
        let outside = tempfile::tempdir().expect("outside");
        let target = outside.path().join("edit.txt");
        std::fs::write(&target, "alpha\n").expect("seed");
        let policy = sandbox_policy(work.path(), crate::sandbox::SandboxMode::WorkspaceWrite);
        let grants = crate::sandbox::SandboxWriteGrants::default();
        let handler = RecordingApprovalHandler::new(vec![
            crate::types::PermissionApprovalDecision::allow_once(),
        ]);
        let runtime =
            permission_runtime(work.path(), policy.clone(), grants.clone(), handler.clone());
        let edit = wrapped_edit(work.path(), policy, grants, &runtime);

        let output = edit
            .execute(
                "call-edit-once".to_string(),
                json!({
                    "mode": "replace",
                    "path": target.display().to_string(),
                    "old_string": "alpha",
                    "new_string": "beta"
                }),
                abort_signal(),
            )
            .await;

        assert!(!output.is_error, "{:?}", output.json);
        assert_eq!(std::fs::read_to_string(&target).expect("target"), "beta\n");
        let requests = handler.requests();
        assert_eq!(requests.len(), 1);
        assert!(!requests[0].allow_always);
    }

    #[tokio::test]
    async fn sandbox_prompts_even_when_permission_profile_allows_path() {
        let work = tempfile::tempdir().expect("work");
        let outside = tempfile::tempdir().expect("outside");
        let target = outside.path().join("profile-allowed.txt");
        let policy = sandbox_policy(work.path(), crate::sandbox::SandboxMode::WorkspaceWrite);
        let grants = crate::sandbox::SandboxWriteGrants::default();
        let handler = RecordingApprovalHandler::new(vec![
            crate::types::PermissionApprovalDecision::allow_once(),
        ]);
        let config = PermissionConfig {
            default_permissions: ":danger-full-access".to_string(),
            ..PermissionConfig::default()
        };
        let runtime = permission_runtime_with_config(
            work.path(),
            config,
            policy.clone(),
            grants.clone(),
            handler.clone(),
        );
        let write = wrapped_write(work.path(), policy, grants, &runtime);

        let output = write
            .execute(
                "call-profile-allowed".to_string(),
                json!({"path": target.display().to_string(), "content": "ok\n"}),
                abort_signal(),
            )
            .await;

        assert!(!output.is_error, "{:?}", output.json);
        assert_eq!(std::fs::read_to_string(&target).expect("target"), "ok\n");
        let requests = handler.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].reason,
            format!(
                "sandbox approval required: write to {} is outside configured writable roots",
                target.display()
            )
        );
        assert!(!requests[0].allow_always);
    }

    #[tokio::test]
    async fn approval_policy_never_blocks_sandbox_widening_prompt() {
        let work = tempfile::tempdir().expect("work");
        let outside = tempfile::tempdir().expect("outside");
        let target = outside.path().join("blocked-by-never.txt");
        let policy = sandbox_policy(work.path(), crate::sandbox::SandboxMode::WorkspaceWrite);
        let grants = crate::sandbox::SandboxWriteGrants::default();
        let handler = RecordingApprovalHandler::new(vec![
            crate::types::PermissionApprovalDecision::allow_once(),
        ]);
        let config = PermissionConfig {
            default_permissions: ":danger-full-access".to_string(),
            approval_policy: ApprovalPolicy::Never,
            ..PermissionConfig::default()
        };
        let runtime = permission_runtime_with_config(
            work.path(),
            config,
            policy.clone(),
            grants.clone(),
            handler.clone(),
        );
        let write = wrapped_write(work.path(), policy, grants, &runtime);

        let output = write
            .execute(
                "call-never".to_string(),
                json!({"path": target.display().to_string(), "content": "no\n"}),
                abort_signal(),
            )
            .await;

        assert!(output.is_error, "{:?}", output.json);
        assert!(
            output.json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("approval_policy=never")
        );
        assert!(!target.exists());
        assert!(handler.requests().is_empty());
    }
}
