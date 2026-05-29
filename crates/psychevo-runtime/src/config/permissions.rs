#[allow(unused_imports)]
pub(crate) use super::*;

pub fn permission_rules_value(options: &RunOptions, scope: ConfigScope) -> Result<Value> {
    let document = config_show_value(options, scope)?;
    let value = document.get("value").cloned().unwrap_or_else(|| json!({}));
    let config = parse_run_config(value)?;
    Ok(json!({
        "scope": document.get("scope").cloned().unwrap_or(Value::String("effective".to_string())),
        "path": document.get("path").cloned().unwrap_or(Value::Null),
        "sources": document.get("sources").cloned().unwrap_or(Value::Array(Vec::new())),
        "permissions": permission_config_value(&config.permissions),
    }))
}

pub(crate) fn permission_config_value(config: &PermissionConfig) -> Value {
    let profiles = config
        .profiles
        .iter()
        .map(|(name, profile)| {
            let mut value = json!({
                "filesystem": access_map_value(&profile.filesystem),
                "network": {
                    "domains": access_map_value(&profile.network_domains),
                },
                "tools": {
                    "skills": access_map_value(&profile.skill_tools),
                },
            });
            if let Some(extends) = &profile.extends {
                value["extends"] = Value::String(extends.clone());
            }
            (name.clone(), value)
        })
        .collect::<serde_json::Map<_, _>>();
    let rules = config
        .exec_policy
        .rules
        .iter()
        .map(|rule| {
            let mut value = json!({
                "prefix": exec_prefix_value(&rule.prefix),
                "decision": rule.decision.as_str(),
            });
            if let Some(justification) = &rule.justification {
                value["justification"] = Value::String(justification.clone());
            }
            if !rule.match_examples.is_empty() {
                value["match"] = Value::Array(
                    rule.match_examples
                        .iter()
                        .map(|example| Value::String(example.raw.clone()))
                        .collect(),
                );
            }
            if !rule.not_match_examples.is_empty() {
                value["not_match"] = Value::Array(
                    rule.not_match_examples
                        .iter()
                        .map(|example| Value::String(example.raw.clone()))
                        .collect(),
                );
            }
            value
        })
        .collect::<Vec<_>>();
    let host_executables = config
        .exec_policy
        .host_executables
        .iter()
        .map(|host| {
            json!({
                "name": host.name.clone(),
                "paths": host.paths.clone(),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "approval_policy": config.approval_policy.as_str(),
        "approvals_reviewer": config.approvals_reviewer.as_str(),
        "default_permissions": config.default_permissions,
        "granular": config.granular.as_ref().map(|granular| json!({
            "filesystem": granular.filesystem,
            "network": granular.network,
            "exec": granular.exec,
            "mcp": granular.mcp,
            "skill": granular.skill,
            "request_permissions": granular.request_permissions,
        })),
        "auto_review": {
            "model": config.auto_review.model,
            "timeout_secs": config.auto_review.timeout_secs,
            "policy": config.auto_review.policy,
        },
        "allow_login_shell": config.allow_login_shell,
        "profiles": profiles,
        "exec_policy": {
            "rules": rules,
            "host_executables": host_executables,
        },
    })
}

pub(crate) fn access_map_value(values: &BTreeMap<String, PermissionAccess>) -> Value {
    Value::Object(
        values
            .iter()
            .map(|(key, value)| (key.clone(), Value::String(value.as_str().to_string())))
            .collect(),
    )
}

pub(crate) fn exec_prefix_value(prefix: &[ExecPolicyPatternToken]) -> Value {
    Value::Array(
        prefix
            .iter()
            .map(|token| match token {
                ExecPolicyPatternToken::Single(value) => Value::String(value.clone()),
                ExecPolicyPatternToken::Alternatives(values) => Value::Array(
                    values
                        .iter()
                        .map(|value| Value::String(value.clone()))
                        .collect(),
                ),
            })
            .collect(),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRuleMutationResult {
    pub config_path: PathBuf,
    pub kind: String,
    pub rule: String,
    pub changed: bool,
}

pub fn append_local_permission_allow_rule(
    config_dir: PathBuf,
    rule: &str,
) -> Result<PermissionRuleMutationResult> {
    append_local_permission_rule(config_dir, "allow", rule)
}

pub fn append_local_permission_rule(
    config_dir: PathBuf,
    kind: &str,
    rule: &str,
) -> Result<PermissionRuleMutationResult> {
    let kind = validate_permission_rule_kind(kind)?;
    let rule = normalize_permission_rule(rule)?;
    match parse_legacy_rule_for_mutation(&rule) {
        LegacyPermissionMutation::Exec(_) => Err(Error::Config(
            "ExecCommand(...) permission mutations are deprecated; edit [[exec_policy.rules]] in config.toml or allow the command permanently from the approval UI"
                .to_string(),
        )),
        LegacyPermissionMutation::Skill(key) => {
            append_local_skill_grant(config_dir, &key, access_for_legacy_kind(kind), rule)
        }
        LegacyPermissionMutation::Network(host) => {
            append_local_network_grant(config_dir, &host, access_for_legacy_kind(kind), rule)
        }
        LegacyPermissionMutation::Filesystem(path) => {
            append_local_filesystem_grant(config_dir, &path, access_for_legacy_kind(kind), rule)
        }
        LegacyPermissionMutation::Unsupported => Err(Error::Config(
            "permission rule must target ExecCommand, WebFetch, Read, Write, Edit, or skill tools"
                .to_string(),
        )),
    }
}

pub fn append_local_filesystem_grant(
    config_dir: PathBuf,
    path: &str,
    access: PermissionAccess,
    label: String,
) -> Result<PermissionRuleMutationResult> {
    append_local_filesystem_grant_with_extends(config_dir, path, access, label, ":workspace")
}

pub fn append_local_filesystem_grant_with_extends(
    config_dir: PathBuf,
    path: &str,
    access: PermissionAccess,
    label: String,
    fallback_extends: &str,
) -> Result<PermissionRuleMutationResult> {
    mutate_local_config(config_dir, "filesystem", label, |parsed| {
        ensure_local_profile(parsed, fallback_extends)?;
        let profile = local_profile_object_mut(parsed)?;
        let filesystem = object_entry_mut(profile, "filesystem")?;
        set_string_entry(filesystem, path, access.as_str())
    })
}

pub fn append_local_network_grant(
    config_dir: PathBuf,
    host: &str,
    access: PermissionAccess,
    label: String,
) -> Result<PermissionRuleMutationResult> {
    append_local_network_grant_with_extends(config_dir, host, access, label, ":workspace")
}

pub fn append_local_network_grant_with_extends(
    config_dir: PathBuf,
    host: &str,
    access: PermissionAccess,
    label: String,
    fallback_extends: &str,
) -> Result<PermissionRuleMutationResult> {
    mutate_local_config(config_dir, "network", label, |parsed| {
        ensure_local_profile(parsed, fallback_extends)?;
        let profile = local_profile_object_mut(parsed)?;
        let network = object_entry_mut(profile, "network")?;
        let domains = object_entry_mut(network, "domains")?;
        set_string_entry(domains, host, access.as_str())
    })
}

pub fn append_local_skill_grant(
    config_dir: PathBuf,
    key: &str,
    access: PermissionAccess,
    label: String,
) -> Result<PermissionRuleMutationResult> {
    append_local_skill_grant_with_extends(config_dir, key, access, label, ":workspace")
}

pub fn append_local_skill_grant_with_extends(
    config_dir: PathBuf,
    key: &str,
    access: PermissionAccess,
    label: String,
    fallback_extends: &str,
) -> Result<PermissionRuleMutationResult> {
    mutate_local_config(config_dir, "skill", label, |parsed| {
        ensure_local_profile(parsed, fallback_extends)?;
        let profile = local_profile_object_mut(parsed)?;
        let tools = object_entry_mut(profile, "tools")?;
        let skills = object_entry_mut(tools, "skills")?;
        set_string_entry(skills, key, access.as_str())
    })
}

pub fn append_local_exec_policy_rule(
    config_dir: PathBuf,
    prefix: &[String],
    decision: ExecPolicyDecision,
    label: String,
) -> Result<PermissionRuleMutationResult> {
    if prefix.is_empty() {
        return Err(Error::Config(
            "exec policy prefix must not be empty".to_string(),
        ));
    }
    mutate_local_config(config_dir, "exec_policy", label, |parsed| {
        let root = root_object_mut(parsed)?;
        let exec_policy = object_entry_mut(root, "exec_policy")?;
        let rules = exec_policy
            .entry("rules".to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        let rules = rules
            .as_array_mut()
            .ok_or_else(|| Error::Config("exec_policy.rules must be an array".to_string()))?;
        let exists = rules.iter().any(|value| {
            value
                .get("prefix")
                .and_then(Value::as_array)
                .map(|values| exec_prefix_strings_from_value(values))
                .as_ref()
                == Some(&prefix.to_vec())
                && value.get("decision").and_then(Value::as_str) == Some(decision.as_str())
        });
        if exists {
            return Ok(false);
        }
        rules.push(json!({
            "prefix": prefix,
            "decision": decision.as_str(),
        }));
        Ok(true)
    })
}

pub fn remove_local_permission_rule(
    config_dir: PathBuf,
    kind: &str,
    rule: &str,
) -> Result<PermissionRuleMutationResult> {
    let kind = validate_permission_rule_kind(kind)?;
    let rule = normalize_permission_rule(rule)?;
    let mutation = parse_legacy_rule_for_mutation(&rule);
    mutate_local_config(config_dir, kind, rule.clone(), |parsed| {
        match mutation {
        LegacyPermissionMutation::Exec(_) => Err(Error::Config(
            "ExecCommand(...) permission mutations are deprecated; edit [[exec_policy.rules]] in config.toml"
                .to_string(),
        )),
        LegacyPermissionMutation::Skill(ref key) => remove_profile_access(
            parsed,
            &["permissions", "local", "tools", "skills"],
            key,
            access_for_legacy_kind(kind),
        ),
        LegacyPermissionMutation::Network(ref host) => remove_profile_access(
            parsed,
            &["permissions", "local", "network", "domains"],
            host,
            access_for_legacy_kind(kind),
        ),
        LegacyPermissionMutation::Filesystem(ref path) => remove_profile_access(
            parsed,
            &["permissions", "local", "filesystem"],
            path,
            access_for_legacy_kind(kind),
        ),
        LegacyPermissionMutation::Unsupported => Ok(false),
    }
    })
}

pub(crate) fn mutate_local_config<F>(
    config_dir: PathBuf,
    kind: &str,
    rule: String,
    mutate: F,
) -> Result<PermissionRuleMutationResult>
where
    F: FnOnce(&mut Value) -> Result<bool>,
{
    fs::create_dir_all(&config_dir)?;
    let config_path = config_dir.join(CONFIG_FILE_NAME);
    let mut parsed = load_toml_config_file(&config_path, false)?;
    let changed = mutate(&mut parsed)?;
    if changed {
        write_toml_config_file(&config_path, &parsed)?;
    }
    Ok(PermissionRuleMutationResult {
        config_path,
        kind: kind.to_string(),
        rule,
        changed,
    })
}

pub(crate) fn root_object_mut(value: &mut Value) -> Result<&mut serde_json::Map<String, Value>> {
    if !value.is_object() {
        *value = json!({});
    }
    value
        .as_object_mut()
        .ok_or_else(|| Error::Config("config root must be an object".to_string()))
}

pub(crate) fn object_entry_mut<'a>(
    object: &'a mut serde_json::Map<String, Value>,
    key: &str,
) -> Result<&'a mut serde_json::Map<String, Value>> {
    let value = object
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    value
        .as_object_mut()
        .ok_or_else(|| Error::Config(format!("{key} must be an object")))
}

pub(crate) fn ensure_local_profile(value: &mut Value, fallback_extends: &str) -> Result<()> {
    let root = root_object_mut(value)?;
    root.insert(
        "default_permissions".to_string(),
        Value::String("local".to_string()),
    );
    let permissions = object_entry_mut(root, "permissions")?;
    let local = object_entry_mut(permissions, "local")?;
    local
        .entry("extends".to_string())
        .or_insert_with(|| Value::String(fallback_extends.to_string()));
    Ok(())
}

pub(crate) fn local_profile_object_mut(
    value: &mut Value,
) -> Result<&mut serde_json::Map<String, Value>> {
    value
        .get_mut("permissions")
        .and_then(Value::as_object_mut)
        .and_then(|permissions| permissions.get_mut("local"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| Error::Config("permissions.local must be an object".to_string()))
}

pub(crate) fn set_string_entry(
    object: &mut serde_json::Map<String, Value>,
    key: &str,
    value: &str,
) -> Result<bool> {
    let next = Value::String(value.to_string());
    if object.get(key) == Some(&next) {
        return Ok(false);
    }
    object.insert(key.to_string(), next);
    Ok(true)
}

#[allow(dead_code)]
pub(crate) fn remove_exec_policy_rule(
    parsed: &mut Value,
    prefix: &[String],
    decision: ExecPolicyDecision,
) -> Result<bool> {
    let Some(rules) = parsed
        .get_mut("exec_policy")
        .and_then(Value::as_object_mut)
        .and_then(|value| value.get_mut("rules"))
        .and_then(Value::as_array_mut)
    else {
        return Ok(false);
    };
    let before = rules.len();
    rules.retain(|value| {
        let same_prefix = value
            .get("prefix")
            .and_then(Value::as_array)
            .map(|values| exec_prefix_strings_from_value(values))
            .as_ref()
            == Some(&prefix.to_vec());
        let same_decision =
            value.get("decision").and_then(Value::as_str) == Some(decision.as_str());
        !(same_prefix && same_decision)
    });
    Ok(rules.len() != before)
}

pub(crate) fn exec_prefix_strings_from_value(values: &[Value]) -> Vec<String> {
    values
        .iter()
        .filter_map(|value| match value {
            Value::String(raw) => Some(raw.clone()),
            Value::Array(alternatives) => alternatives
                .first()
                .and_then(Value::as_str)
                .map(str::to_string),
            _ => None,
        })
        .collect()
}

pub(crate) fn remove_profile_access(
    parsed: &mut Value,
    path: &[&str],
    key: &str,
    access: PermissionAccess,
) -> Result<bool> {
    let Some(object) = nested_object_mut(parsed, path) else {
        return Ok(false);
    };
    let Some(value) = object.get(key).and_then(Value::as_str) else {
        return Ok(false);
    };
    if value != access.as_str() {
        return Ok(false);
    }
    object.remove(key);
    Ok(true)
}

pub(crate) fn nested_object_mut<'a>(
    value: &'a mut Value,
    path: &[&str],
) -> Option<&'a mut serde_json::Map<String, Value>> {
    let mut current = value.as_object_mut()?;
    for key in path {
        current = current.get_mut(*key)?.as_object_mut()?;
    }
    Some(current)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LegacyPermissionMutation {
    Exec(Vec<String>),
    Filesystem(String),
    Network(String),
    Skill(String),
    Unsupported,
}

pub(crate) fn parse_legacy_rule_for_mutation(rule: &str) -> LegacyPermissionMutation {
    let Some((tool, pattern)) = legacy_rule_parts(rule) else {
        return LegacyPermissionMutation::Unsupported;
    };
    match tool {
        "ExecCommand" | "exec_command" => {
            LegacyPermissionMutation::Exec(exec_prefix_from_command(pattern))
        }
        "Read" | "read" | "Write" | "write" | "Edit" | "edit" => {
            LegacyPermissionMutation::Filesystem(pattern.to_string())
        }
        "WebFetch" | "web_fetch" => web_fetch_host(pattern)
            .map(LegacyPermissionMutation::Network)
            .unwrap_or(LegacyPermissionMutation::Unsupported),
        "SkillManage" | "skill_manage" => {
            LegacyPermissionMutation::Skill(format!("skill_manage/{pattern}"))
        }
        "SkillHub" | "skill_hub" => LegacyPermissionMutation::Skill(format!("skill_hub/{pattern}")),
        "SkillConfig" | "skill_config" => {
            LegacyPermissionMutation::Skill(format!("skill_config/{pattern}"))
        }
        _ => LegacyPermissionMutation::Unsupported,
    }
}

pub(crate) fn legacy_rule_parts(rule: &str) -> Option<(&str, &str)> {
    let (tool, rest) = rule.trim().split_once('(')?;
    let pattern = rest.strip_suffix(')')?.trim();
    Some((tool.trim(), pattern))
}

pub(crate) fn exec_prefix_from_command(command: &str) -> Vec<String> {
    command
        .split_whitespace()
        .take_while(|token| *token != "*")
        .map(str::to_string)
        .collect()
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

#[allow(dead_code)]
pub(crate) fn permission_decision_for_legacy_kind(kind: &str) -> ExecPolicyDecision {
    match kind {
        "allow" => ExecPolicyDecision::Allow,
        "ask" => ExecPolicyDecision::Prompt,
        "deny" => ExecPolicyDecision::Deny,
        _ => ExecPolicyDecision::Prompt,
    }
}

pub(crate) fn access_for_legacy_kind(kind: &str) -> PermissionAccess {
    match kind {
        "allow" => PermissionAccess::Allow,
        "ask" => PermissionAccess::Prompt,
        "deny" => PermissionAccess::Deny,
        _ => PermissionAccess::Prompt,
    }
}

pub(crate) fn validate_permission_rule_kind(kind: &str) -> Result<&'static str> {
    match kind {
        "allow" => Ok("allow"),
        "ask" => Ok("ask"),
        "deny" => Ok("deny"),
        _ => Err(Error::Config(
            "permission rule kind must be allow, ask, or deny".to_string(),
        )),
    }
}

pub(crate) fn normalize_permission_rule(rule: &str) -> Result<String> {
    let rule = rule.trim();
    if rule.is_empty() {
        return Err(Error::Config(
            "permission rule must not be empty".to_string(),
        ));
    }
    Ok(rule.to_string())
}

#[cfg(test)]
pub(crate) mod permission_rule_tests {
    pub(crate) use super::*;

    #[test]
    fn legacy_exec_permission_mutation_is_deprecated() {
        let temp = tempfile::tempdir().expect("temp");
        let config_dir = temp.path().join(".psychevo");

        let err = append_local_permission_allow_rule(config_dir, "ExecCommand(cargo test *)")
            .expect_err("legacy exec mutation should fail");
        assert!(err.to_string().contains("[[exec_policy.rules]]"));
    }

    #[test]
    fn exec_policy_rule_mutations_write_new_schema_and_skip_duplicates() {
        let temp = tempfile::tempdir().expect("temp");
        let config_dir = temp.path().join(".psychevo");

        let prefix = vec!["cargo".to_string(), "test".to_string()];
        let added = append_local_exec_policy_rule(
            config_dir.clone(),
            &prefix,
            ExecPolicyDecision::Allow,
            "exec:cargo test".to_string(),
        )
        .expect("append");
        assert!(added.changed);
        let duplicate = append_local_exec_policy_rule(
            config_dir.clone(),
            &prefix,
            ExecPolicyDecision::Allow,
            "exec:cargo test".to_string(),
        )
        .expect("duplicate append");
        assert!(!duplicate.changed);

        let config_path = config_dir.join(CONFIG_FILE_NAME);
        let text = fs::read_to_string(&config_path).expect("config");
        assert!(text.contains("[[exec_policy.rules]]"));
        assert!(text.contains("\"cargo\""));
        assert!(text.contains("\"test\""));
        assert!(text.contains("decision = \"allow\""));
    }

    #[test]
    fn filesystem_grants_create_local_profile() {
        let temp = tempfile::tempdir().expect("temp");
        let config_dir = temp.path().join(".psychevo");
        let result = append_local_filesystem_grant(
            config_dir.clone(),
            "/tmp/shared",
            PermissionAccess::Read,
            "/tmp/shared".to_string(),
        )
        .expect("append");
        assert!(result.changed);
        let text = fs::read_to_string(config_dir.join(CONFIG_FILE_NAME)).expect("config");
        assert!(text.contains("default_permissions = \"local\""));
        assert!(text.contains("[permissions.local]"));
        assert!(text.contains("extends = \":workspace\""));
        assert!(text.contains("\"/tmp/shared\" = \"read\""));
    }

    #[test]
    fn parses_new_permission_schema() {
        let config = parse_run_config(json!({
            "approval_policy": "granular",
            "approvals_reviewer": "smart",
            "default_permissions": "local",
            "approval": {
                "granular": {
                    "filesystem": true,
                    "network": true,
                    "exec": true,
                    "mcp": true,
                    "skill": true,
                    "request_permissions": false,
                }
            },
            "auto_review": {
                "model": "mock/reviewer",
                "timeout_secs": 5,
            },
            "permissions": {
                "allow_login_shell": true,
                "local": {
                    "extends": ":workspace",
                    "filesystem": {
                        "/tmp/shared": "read",
                    },
                    "network": {
                        "domains": {
                            "example.com": "allow",
                        },
                    },
                    "tools": {
                        "skills": {
                            "skill_manage/install": "prompt",
                        },
                    },
                },
            },
            "exec_policy": {
                "rules": [
                    {
                        "prefix": ["git", ["status", "diff"]],
                        "decision": "allow",
                        "justification": "read-only git inspection",
                        "match": ["git status --short"],
                        "not_match": ["git push"],
                    },
                ],
                "host_executables": [
                    {
                        "name": "git",
                        "paths": ["/usr/bin/git"],
                    },
                ],
            },
        }))
        .expect("config");

        assert_eq!(config.permissions.approval_policy, ApprovalPolicy::Granular);
        assert_eq!(
            config.permissions.approvals_reviewer,
            ApprovalsReviewer::Smart
        );
        assert_eq!(config.permissions.default_permissions, "local");
        assert!(config.permissions.allow_login_shell);
        assert_eq!(config.permissions.auto_review.timeout_secs, 5);
        assert_eq!(config.permissions.exec_policy.rules.len(), 1);
        assert_eq!(
            config.permissions.exec_policy.host_executables[0].paths,
            vec!["/usr/bin/git".to_string()]
        );
        let local = config.permissions.profiles.get("local").expect("local");
        assert_eq!(
            local.filesystem.get("/tmp/shared"),
            Some(&PermissionAccess::Read)
        );
    }

    #[test]
    fn legacy_permission_schema_is_a_hard_error() {
        let err = parse_run_config(json!({
            "permissions": {
                "allow": ["ExecCommand(cargo test *)"],
            }
        }))
        .expect_err("legacy schema should fail");
        assert!(err.to_string().contains("permissions.allow is deprecated"));
    }

    #[test]
    fn granular_policy_requires_explicit_matrix() {
        let err = parse_run_config(json!({
            "approval_policy": "granular",
        }))
        .expect_err("granular without matrix should fail");
        assert!(err.to_string().contains("requires [approval.granular]"));
    }

    #[test]
    fn exec_policy_self_test_failures_are_config_errors() {
        let err = parse_run_config(json!({
            "exec_policy": {
                "rules": [
                    {
                        "prefix": ["git", "status"],
                        "decision": "allow",
                        "match": ["git push"],
                    },
                ],
            },
        }))
        .expect_err("bad match example should fail");
        assert!(err.to_string().contains("does not match prefix"));
    }
}
