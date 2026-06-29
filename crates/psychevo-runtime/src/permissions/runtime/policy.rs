impl PermissionRuntime {
    pub(crate) fn evaluate(&self, tool_name: &str, args: &Value) -> PermissionDecision {
        let action = PermissionAction::from_tool_call(&self.inner.cwd, tool_name, args);
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
                    && action.file_targets_all_within_cwd()
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
            cwd,
        } = action
        else {
            return None;
        };
        if cwd
            .as_ref()
            .is_some_and(|target| !target.within_cwd)
        {
            return Some(ActionPolicyEvaluation::Ask {
                reason: "command cwd outside accepted cwd requires approval".to_string(),
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
            .map(|path| file_target(&self.inner.cwd, path))
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
