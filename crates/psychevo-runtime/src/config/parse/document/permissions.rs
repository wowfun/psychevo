pub(crate) fn parse_permission_config(
    root: &serde_json::Map<String, Value>,
) -> Result<PermissionConfig> {
    reject_legacy_permission_keys(root)?;
    let mut config = PermissionConfig::default();
    if let Some(value) = optional_string_field(root, "approval_policy")? {
        if value == "on-failure" || value == "on_failure" {
            return Err(Error::Config(
                "approval_policy = \"on-failure\" is not supported; use on-request, untrusted, never, or granular"
                    .to_string(),
            ));
        }
        config.approval_policy = ApprovalPolicy::parse(&value).ok_or_else(|| {
            Error::Config(
                "approval_policy must be on-request, untrusted, never, or granular".to_string(),
            )
        })?;
    }
    if let Some(value) = optional_string_field(root, "approvals_reviewer")? {
        config.approvals_reviewer = ApprovalsReviewer::parse(&value)
            .ok_or_else(|| Error::Config("approvals_reviewer must be user or smart".to_string()))?;
    }
    if let Some(value) = optional_string_field(root, "default_permissions")? {
        validate_permission_profile_name(&value)?;
        config.default_permissions = value;
    }
    if let Some(auto_review) = root.get("auto_review") {
        config.auto_review = parse_auto_review_config(auto_review)?;
    }
    if let Some(approval) = root.get("approval") {
        config.granular = parse_approval_config(approval)?;
    }
    if matches!(config.approval_policy, ApprovalPolicy::Granular) && config.granular.is_none() {
        return Err(Error::Config(
            "approval_policy = \"granular\" requires [approval.granular] with filesystem, network, exec, mcp, skill, and request_permissions"
                .to_string(),
        ));
    }
    if let Some(permissions) = root.get("permissions") {
        let permissions = permissions
            .as_object()
            .ok_or_else(|| Error::Config("permissions must be an object".to_string()))?;
        config.allow_login_shell =
            optional_bool_field(permissions, "allow_login_shell")?.unwrap_or(false);
        for (name, value) in permissions {
            if name == "allow_login_shell" {
                continue;
            }
            validate_permission_profile_name(name)?;
            config
                .profiles
                .insert(name.clone(), parse_permission_profile(name, value)?);
        }
    }
    if let Some(exec_policy) = root.get("exec_policy") {
        config.exec_policy = parse_exec_policy_config(exec_policy)?;
    }
    Ok(config)
}

pub(crate) fn reject_legacy_permission_keys(root: &serde_json::Map<String, Value>) -> Result<()> {
    for key in [
        "permission_mode",
        "permissionMode",
        "approval_mode",
        "approvalMode",
    ] {
        if root.contains_key(key) {
            return Err(Error::Config(format!(
                "{key} is deprecated; use approval_policy, approvals_reviewer, default_permissions, and [permissions.<profile>]"
            )));
        }
    }
    let Some(permissions) = root.get("permissions").and_then(Value::as_object) else {
        return Ok(());
    };
    for key in [
        "permission_mode",
        "permissionMode",
        "approval_mode",
        "approvalMode",
        "smart_model",
        "smartModel",
        "allow",
        "ask",
        "deny",
    ] {
        if permissions.contains_key(key) {
            return Err(Error::Config(format!(
                "permissions.{key} is deprecated; use approval_policy, approvals_reviewer, [permissions.<profile>], and [[exec_policy.rules]]"
            )));
        }
    }
    Ok(())
}

pub(crate) fn parse_auto_review_config(value: &Value) -> Result<AutoReviewConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("auto_review must be an object".to_string()))?;
    Ok(AutoReviewConfig {
        model: optional_string_field(object, "model")?,
        timeout_secs: optional_u64_field(object, "timeout_secs")?.unwrap_or(90),
        policy: optional_string_field(object, "policy")?,
    })
}

pub(crate) fn parse_approval_config(value: &Value) -> Result<Option<GranularApprovalConfig>> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("approval must be an object".to_string()))?;
    let Some(granular) = object.get("granular") else {
        return Ok(None);
    };
    let granular = granular
        .as_object()
        .ok_or_else(|| Error::Config("approval.granular must be an object".to_string()))?;
    Ok(Some(GranularApprovalConfig {
        filesystem: required_bool_field(granular, "filesystem", "approval.granular.filesystem")?,
        network: required_bool_field(granular, "network", "approval.granular.network")?,
        exec: required_bool_field(granular, "exec", "approval.granular.exec")?,
        mcp: required_bool_field(granular, "mcp", "approval.granular.mcp")?,
        skill: required_bool_field(granular, "skill", "approval.granular.skill")?,
        request_permissions: required_bool_field(
            granular,
            "request_permissions",
            "approval.granular.request_permissions",
        )?,
    }))
}

pub(crate) fn parse_permission_profile(
    name: &str,
    value: &Value,
) -> Result<PermissionProfileConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config(format!("permissions.{name} must be an object")))?;
    Ok(PermissionProfileConfig {
        extends: optional_string_field(object, "extends")?,
        filesystem: object
            .get("filesystem")
            .map(|value| parse_access_map(value, &format!("permissions.{name}.filesystem")))
            .transpose()?
            .unwrap_or_default(),
        network_domains: object
            .get("network")
            .map(|value| parse_network_domains(value, name))
            .transpose()?
            .unwrap_or_default(),
        web_search_queries: object
            .get("web_search")
            .map(|value| parse_web_search_queries(value, name))
            .transpose()?
            .unwrap_or_default(),
        skill_tools: object
            .get("tools")
            .map(|value| parse_tool_grants(value, name))
            .transpose()?
            .unwrap_or_default(),
    })
}

pub(crate) fn parse_web_search_queries(
    value: &Value,
    profile: &str,
) -> Result<BTreeMap<String, PermissionAccess>> {
    let object = value.as_object().ok_or_else(|| {
        Error::Config(format!("permissions.{profile}.web_search must be an object"))
    })?;
    object
        .get("queries")
        .map(|value| parse_access_map(value, &format!("permissions.{profile}.web_search.queries")))
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(crate) fn parse_network_domains(
    value: &Value,
    profile: &str,
) -> Result<BTreeMap<String, PermissionAccess>> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config(format!("permissions.{profile}.network must be an object")))?;
    object
        .get("domains")
        .map(|value| parse_access_map(value, &format!("permissions.{profile}.network.domains")))
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(crate) fn parse_tool_grants(
    value: &Value,
    profile: &str,
) -> Result<BTreeMap<String, PermissionAccess>> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config(format!("permissions.{profile}.tools must be an object")))?;
    object
        .get("skills")
        .map(|value| parse_access_map(value, &format!("permissions.{profile}.tools.skills")))
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(crate) fn parse_access_map(
    value: &Value,
    path: &str,
) -> Result<BTreeMap<String, PermissionAccess>> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config(format!("{path} must be an object")))?;
    let mut out = BTreeMap::new();
    for (key, value) in object {
        let raw = value
            .as_str()
            .map(str::trim)
            .ok_or_else(|| Error::Config(format!("{path}.{key} must be a string")))?;
        let access = PermissionAccess::parse(raw).ok_or_else(|| {
            Error::Config(format!(
                "{path}.{key} must be deny, read, write, allow, or prompt"
            ))
        })?;
        out.insert(key.clone(), access);
    }
    Ok(out)
}
