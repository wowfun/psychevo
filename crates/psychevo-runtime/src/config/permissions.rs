use jsonc_parser::cst::{CstLeafNode, CstNode};

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
    let config_path = config_dir.join("config.jsonc");
    let text = if config_path.exists() {
        fs::read_to_string(&config_path)?
    } else {
        "{}\n".to_string()
    };
    let parsed = jsonc_parser::parse_to_serde_value::<Option<Value>>(&text, &Default::default())
        .map_err(|err| Error::Config(format!("{}: {err}", config_path.display())))?
        .unwrap_or_else(|| json!({}));
    if permission_rule_values(&parsed, kind).iter().any(|entry| entry == &rule) {
        return Ok(PermissionRuleMutationResult {
            config_path,
            kind: kind.to_string(),
            rule,
            changed: false,
        });
    }

    let root = CstRootNode::parse(&text, &ParseOptions::default())
        .map_err(|err| Error::Config(format!("{}: {err}", config_path.display())))?;
    let root_object = root.object_value_or_set();
    let permissions = root_object.object_value_or_set("permissions");
    let array = permissions.array_value_or_set(kind);
    array.append(CstInputValue::String(rule.clone()));
    fs::write(&config_path, ensure_trailing_newline(root.to_string()))?;
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
    let config_path = config_dir.join("config.jsonc");
    let text = fs::read_to_string(&config_path).unwrap_or_else(|_| "{}\n".to_string());
    let root = CstRootNode::parse(&text, &ParseOptions::default())
        .map_err(|err| Error::Config(format!("{}: {err}", config_path.display())))?;
    let changed = if let Some(array) = root
        .object_value()
        .and_then(|root| root.object_value("permissions"))
        .and_then(|permissions| permissions.array_value(kind))
    {
        let mut changed = false;
        for element in array.elements() {
            if let CstNode::Leaf(CstLeafNode::StringLit(lit)) = element
                && lit.decoded_value().ok().as_deref() == Some(rule.as_str())
            {
                lit.remove();
                changed = true;
                break;
            }
        }
        changed
    } else {
        false
    };
    if changed {
        fs::write(&config_path, ensure_trailing_newline(root.to_string()))?;
    }
    Ok(PermissionRuleMutationResult {
        config_path,
        kind: kind.to_string(),
        rule,
        changed,
    })
}

fn permission_rule_values(value: &Value, kind: &str) -> Vec<String> {
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

fn validate_permission_rule_kind(kind: &str) -> Result<&'static str> {
    match kind {
        "allow" => Ok("allow"),
        "ask" => Ok("ask"),
        "deny" => Ok("deny"),
        _ => Err(Error::Config(
            "permission rule kind must be allow, ask, or deny".to_string(),
        )),
    }
}

fn normalize_permission_rule(rule: &str) -> Result<String> {
    let rule = rule.trim();
    if rule.is_empty() {
        return Err(Error::Config("permission rule must not be empty".to_string()));
    }
    Ok(rule.to_string())
}

#[cfg(test)]
mod permission_rule_tests {
    use super::*;

    #[test]
    fn permission_rule_mutations_preserve_jsonc_and_skip_duplicates() {
        let temp = tempfile::tempdir().expect("temp");
        let config_dir = temp.path().join(".psychevo");
        fs::create_dir_all(&config_dir).expect("config dir");
        let config_path = config_dir.join("config.jsonc");
        fs::write(
            &config_path,
            r#"{
  // local permission policy
  "permissions": {
    "allow": ["Bash(npm test *)"]
  }
}
"#,
        )
        .expect("config");

        let duplicate =
            append_local_permission_allow_rule(config_dir.clone(), "Bash(npm test *)")
                .expect("duplicate append");
        assert!(!duplicate.changed);

        let added = append_local_permission_allow_rule(config_dir.clone(), "Bash(cargo test *)")
            .expect("append");
        assert!(added.changed);
        let text = fs::read_to_string(&config_path).expect("config");
        assert!(text.contains("local permission policy"));
        assert!(text.contains("Bash(npm test *)"));
        assert!(text.contains("Bash(cargo test *)"));

        let removed = remove_local_permission_rule(config_dir, "allow", "Bash(cargo test *)")
            .expect("remove");
        assert!(removed.changed);
        let text = fs::read_to_string(config_path).expect("config");
        assert!(text.contains("local permission policy"));
        assert!(!text.contains("Bash(cargo test *)"));
    }
}
