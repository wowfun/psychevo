pub(crate) fn hardline_deny(action: &PermissionAction) -> Option<String> {
    match action {
        PermissionAction::ExecCommand {
            command,
            normalized,
            ..
        } => {
            background_shell_reason(command).or_else(|| hardline_bash_reason(normalized))
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
        PermissionAction::WebSearch { .. } => None,
    }
}

pub(crate) fn default_ask_reason(action: &PermissionAction) -> Option<String> {
    match action {
        PermissionAction::ExecCommand {
            normalized,
            cwd,
            ..
        } => {
            if cwd
                .as_ref()
                .is_some_and(|target| !target.within_cwd)
            {
                return Some(
                    "command cwd outside accepted cwd requires approval".to_string(),
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
        PermissionAction::WebSearch { .. } => None,
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
            if paths.iter().all(|target| target.within_cwd) {
                return ActionPolicyEvaluation::Allow;
            }
            let outside = paths
                .iter()
                .filter(|target| !target.within_cwd)
                .map(|target| target.absolute.to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            ActionPolicyEvaluation::Ask {
                reason: format!(
                    "{} outside cwd requires approval: {outside}",
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
        PermissionAction::WebSearch { .. } => ActionPolicyEvaluation::Allow,
    }
}

pub(crate) fn read_only_profile_decision(action: &PermissionAction) -> ActionPolicyEvaluation {
    match action {
        PermissionAction::File {
            paths,
            mutating: false,
            ..
        } if paths.iter().all(|target| target.within_cwd) => ActionPolicyEvaluation::Allow,
        PermissionAction::File { .. } => ActionPolicyEvaluation::Ask {
            reason:
                "read-only permissions require approval for file writes or outside-cwd reads"
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
        PermissionAction::WebSearch { query } => profile_access_decision(
            profile_name,
            "web_search.queries",
            &profile.web_search_queries,
            query,
            || format!("web search for `{query}` requires approval"),
            || action.persistent_grants(),
        ),
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
        let rule_ref = crate::host_paths::path_ref_for_native_path(&normalized);
        return path_uri_contains(&rule_ref.uri, &target.uri);
    }
    let rule = rule.replace('\\', "/");
    target.relative == rule
        || target
            .relative
            .strip_prefix(&rule)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn path_uri_contains(root: &str, target: &str) -> bool {
    let root = root.trim_end_matches('/');
    target == root || target.strip_prefix(root).is_some_and(|rest| rest.starts_with('/'))
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

#[cfg(test)]
mod profile_filesystem_tests {
    use super::*;

    #[test]
    fn relative_filesystem_rule_matches_decoded_space_path() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path();
        let path = cwd.join("a b.txt");
        let path_ref = crate::host_paths::path_ref_for_native_path(&path);
        let target = file_target(cwd, &path_ref.uri);

        assert!(filesystem_rule_matches("a b.txt", &target));
        assert!(!filesystem_rule_matches("a%20b.txt", &target));
    }
}
