impl PermissionRuntime {
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
}
