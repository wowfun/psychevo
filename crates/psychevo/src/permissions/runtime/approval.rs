impl PermissionRuntime {
    async fn authorize_sandbox_write_grant(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        args: &Value,
        grant: SandboxWriteGrantRequest,
        abort: Option<AbortSignal>,
    ) -> std::result::Result<(), ToolOutput> {
        let action = PermissionAction::from_tool_call(&self.inner.cwd, tool_name, args)
            .map_err(|err| ToolOutput::error(err.to_string()))?;
        if self.inner.mode.bypasses_prompt_asks() {
            return Err(ToolOutput::error(format!(
                "denied by sandbox policy: {}; bypassPermissions does not bypass sandbox enforcement",
                grant.reason
            )));
        }
        if let Some(action) = action.as_ref()
            && matches!(self.inner.config.approval_policy, ApprovalPolicy::Granular)
            && !self.granular_allows_prompt(action)
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
                filesystem: action
                    .as_ref()
                    .and_then(PermissionAction::filesystem_approval_request),
                abort,
            })
            .await?;
        if self.inner.config.approvals_reviewer == ApprovalsReviewer::Smart {
            return match decision.outcome {
                PermissionApprovalOutcome::AllowOnce
                | PermissionApprovalOutcome::AllowTurn
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
            PermissionApprovalOutcome::AllowTurn | PermissionApprovalOutcome::AllowSession => {
                let scope = decision
                    .filesystem_scope
                    .as_ref()
                    .expect("validated filesystem scope");
                self.remember_filesystem_scope(scope)
                    .map_err(|err| ToolOutput::error(err.to_string()))?;
                self.inner
                    .sandbox_grants
                    .grant_once(tool_call_id, &grant.paths)
                    .map_err(|err| ToolOutput::error(err.to_string()))?;
                Ok(())
            }
            PermissionApprovalOutcome::AllowAlways => Err(permission_error(
                "denied",
                "filesystem approval cannot be persisted from a tool-call prompt",
                None,
            )),
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
            filesystem,
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
        if let Some(hook_runtime) = &self.inner.hook_runtime {
            let hook_outcome = hook_runtime.run_permission_request(
                &json!({
                    "tool": tool_name,
                    "tool_call_id": tool_call_id,
                    "arguments": args,
                    "reason": reason,
                    "matched_rule": matched_rule,
                    "suggested_rule": suggested_rule,
                    "allow_always": false,
                    "filesystem": filesystem.clone(),
                }),
            );
            if let Some(decision) = hook_outcome.approval_decision() {
                if decision.outcome == PermissionApprovalOutcome::Deny {
                    let hook_reason = hook_outcome
                        .response
                        .feedback
                        .first()
                        .cloned()
                        .or(hook_outcome.response.blocked_reason)
                        .or_else(|| hook_outcome.response.diagnostics.first().cloned())
                        .unwrap_or_else(|| "PermissionRequest hook denied permission".to_string());
                    return Err(permission_error(
                        "denied",
                        &format!("PermissionRequest hook denied permission: {hook_reason}"),
                        matched_rule,
                    ));
                }
                return Ok(crate::types::PermissionApprovalDecision::allow_once());
            }
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
            filesystem,
            timeout_secs,
        };
        let mut pending_approval = self.start_pending_approval(&request);
        let decision = match abort {
            Some(mut abort) if timeout_secs == 0 => {
                tokio::select! {
                    biased;
                    _ = abort.wait_for_abort() => return Err(ToolOutput::error("aborted")),
                    decision = handler.request_permission(request.clone()) => decision,
                }
            }
            Some(mut abort) => {
                tokio::select! {
                    biased;
                    _ = abort.wait_for_abort() => return Err(ToolOutput::error("aborted")),
                    decision = time::timeout(
                        Duration::from_secs(timeout_secs),
                        handler.request_permission(request.clone()),
                    ) => {
                        decision.unwrap_or_else(|_| crate::types::PermissionApprovalDecision::deny())
                    }
                }
            }
            None if timeout_secs == 0 => handler.request_permission(request.clone()).await,
            None => time::timeout(
                Duration::from_secs(timeout_secs),
                handler.request_permission(request.clone()),
            )
            .await
            .unwrap_or_else(|_| crate::types::PermissionApprovalDecision::deny()),
        };
        let decision = validate_approval_decision(&request, decision);
        pending_approval.finish(decision.outcome);
        Ok(decision)
    }
}

fn validate_approval_decision(
    request: &PermissionApprovalRequest,
    decision: crate::types::PermissionApprovalDecision,
) -> crate::types::PermissionApprovalDecision {
    let Some(filesystem) = &request.filesystem else {
        return if decision.outcome == PermissionApprovalOutcome::AllowTurn
            || decision.filesystem_scope.is_some()
        {
            crate::types::PermissionApprovalDecision::deny()
        } else {
            decision
        };
    };
    match decision.outcome {
        PermissionApprovalOutcome::AllowOnce | PermissionApprovalOutcome::Deny => {
            if decision.filesystem_scope.is_none() {
                decision
            } else {
                crate::types::PermissionApprovalDecision::deny()
            }
        }
        PermissionApprovalOutcome::AllowTurn | PermissionApprovalOutcome::AllowSession => {
            let Some(scope) = decision.filesystem_scope.as_ref() else {
                return crate::types::PermissionApprovalDecision::deny();
            };
            let lifetime_matches = matches!(
                (decision.outcome, scope.lifetime),
                (
                    PermissionApprovalOutcome::AllowTurn,
                    FilesystemApprovalLifetime::Turn
                ) | (
                    PermissionApprovalOutcome::AllowSession,
                    FilesystemApprovalLifetime::Session
                )
            );
            if lifetime_matches && filesystem.scope_candidates.contains(&scope.directory) {
                decision
            } else {
                crate::types::PermissionApprovalDecision::deny()
            }
        }
        PermissionApprovalOutcome::AllowAlways => {
            crate::types::PermissionApprovalDecision::deny()
        }
    }
}
