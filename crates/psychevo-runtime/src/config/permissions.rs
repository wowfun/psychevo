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
        "permissions": {
            "approval_mode": config.permissions.approval_mode.map(|mode| mode.as_str()),
            "permission_mode": config.permissions.permission_mode.map(|mode| mode.as_str()),
            "smart_model": config.permissions.smart_model,
            "allow_login_shell": config.permissions.allow_login_shell,
            "allow": config.permissions.allow,
            "ask": config.permissions.ask,
            "deny": config.permissions.deny,
        }
    }))
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
    fs::create_dir_all(&config_dir)?;
    let config_path = config_dir.join(CONFIG_FILE_NAME);
    let mut parsed = load_toml_config_file(&config_path, false)?;
    if permission_rule_values(&parsed, kind)
        .iter()
        .any(|entry| entry == &rule)
    {
        return Ok(PermissionRuleMutationResult {
            config_path,
            kind: kind.to_string(),
            rule,
            changed: false,
        });
    }

    permission_array_mut(&mut parsed, kind)?.push(Value::String(rule.clone()));
    write_toml_config_file(&config_path, &parsed)?;
    Ok(PermissionRuleMutationResult {
        config_path,
        kind: kind.to_string(),
        rule,
        changed: true,
    })
}

pub fn remove_local_permission_rule(
    config_dir: PathBuf,
    kind: &str,
    rule: &str,
) -> Result<PermissionRuleMutationResult> {
    let kind = validate_permission_rule_kind(kind)?;
    let rule = normalize_permission_rule(rule)?;
    let config_path = config_dir.join(CONFIG_FILE_NAME);
    let mut parsed = load_toml_config_file(&config_path, false)?;
    let mut changed = false;
    if let Some(array) = parsed
        .get_mut("permissions")
        .and_then(Value::as_object_mut)
        .and_then(|permissions| permissions.get_mut(kind))
        .and_then(Value::as_array_mut)
        && let Some(index) = array
            .iter()
            .position(|value| value.as_str() == Some(rule.as_str()))
    {
        array.remove(index);
        changed = true;
    }
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

pub(crate) fn permission_rule_values(value: &Value, kind: &str) -> Vec<String> {
    value
        .get("permissions")
        .and_then(Value::as_object)
        .and_then(|permissions| permissions.get(kind))
        .and_then(Value::as_array)
        .map(|rules| {
            rules
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
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

pub(crate) fn permission_array_mut<'a>(
    value: &'a mut Value,
    kind: &str,
) -> Result<&'a mut Vec<Value>> {
    if !value.is_object() {
        *value = json!({});
    }
    let root = value
        .as_object_mut()
        .ok_or_else(|| Error::Config("config root must be an object".to_string()))?;
    let permissions = root
        .entry("permissions".to_string())
        .or_insert_with(|| json!({}));
    if !permissions.is_object() {
        return Err(Error::Config("permissions must be an object".to_string()));
    }
    let permissions = permissions
        .as_object_mut()
        .ok_or_else(|| Error::Config("permissions must be an object".to_string()))?;
    let values = permissions
        .entry(kind.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    values
        .as_array_mut()
        .ok_or_else(|| Error::Config(format!("permissions.{kind} must be an array")))
}

#[cfg(test)]
pub(crate) mod permission_rule_tests {
    pub(crate) use super::*;

    #[test]
    fn permission_rule_mutations_write_toml_and_skip_duplicates() {
        let temp = tempfile::tempdir().expect("temp");
        let config_dir = temp.path().join(".psychevo");
        fs::create_dir_all(&config_dir).expect("config dir");
        let config_path = config_dir.join(CONFIG_FILE_NAME);
        fs::write(
            &config_path,
            r#"[permissions]
allow = ["ExecCommand(npm test *)"]
"#,
        )
        .expect("config");

        let duplicate =
            append_local_permission_allow_rule(config_dir.clone(), "ExecCommand(npm test *)")
                .expect("duplicate append");
        assert!(!duplicate.changed);

        let added =
            append_local_permission_allow_rule(config_dir.clone(), "ExecCommand(cargo test *)")
                .expect("append");
        assert!(added.changed);
        let text = fs::read_to_string(&config_path).expect("config");
        assert!(text.contains("ExecCommand(npm test *)"));
        assert!(text.contains("ExecCommand(cargo test *)"));

        let removed =
            remove_local_permission_rule(config_dir, "allow", "ExecCommand(cargo test *)")
                .expect("remove");
        assert!(removed.changed);
        let text = fs::read_to_string(config_path).expect("config");
        assert!(!text.contains("ExecCommand(cargo test *)"));
    }
}
