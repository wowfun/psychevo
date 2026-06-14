impl PermissionRuntime {
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
}
