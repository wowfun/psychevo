pub(crate) fn parse_exec_policy_config(value: &Value) -> Result<ExecPolicyConfig> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Config("exec_policy must be an object".to_string()))?;
    let host_executables = object
        .get("host_executables")
        .map(parse_host_executables)
        .transpose()?
        .unwrap_or_default();
    let rules = object
        .get("rules")
        .map(|value| {
            value
                .as_array()
                .ok_or_else(|| Error::Config("exec_policy.rules must be an array".to_string()))
        })
        .transpose()?
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for (index, value) in rules.iter().enumerate() {
        let object = value.as_object().ok_or_else(|| {
            Error::Config(format!("exec_policy.rules[{index}] must be an object"))
        })?;
        let prefix = exec_policy_prefix_field(
            object.get("prefix"),
            &format!("exec_policy.rules[{index}].prefix"),
        )?;
        if prefix.is_empty() {
            return Err(Error::Config(format!(
                "exec_policy.rules[{index}].prefix must not be empty"
            )));
        }
        let decision = optional_string_field(object, "decision")?
            .and_then(|value| ExecPolicyDecision::parse(&value))
            .ok_or_else(|| {
                Error::Config(format!(
                    "exec_policy.rules[{index}].decision must be allow, prompt, or deny"
                ))
            })?;
        let justification = optional_string_field(object, "justification")?;
        let match_examples = exec_policy_examples_field(
            object.get("match"),
            &format!("exec_policy.rules[{index}].match"),
        )?;
        let not_match_examples = exec_policy_examples_field(
            object.get("not_match"),
            &format!("exec_policy.rules[{index}].not_match"),
        )?;
        let rule = ExecPolicyRule {
            prefix,
            decision,
            justification,
            match_examples,
            not_match_examples,
        };
        validate_exec_policy_rule_examples(&rule, index, &host_executables)?;
        out.push(rule);
    }
    Ok(ExecPolicyConfig {
        rules: out,
        host_executables,
    })
}

pub(crate) fn exec_policy_prefix_field(
    value: Option<&Value>,
    path: &str,
) -> Result<Vec<ExecPolicyPatternToken>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .ok_or_else(|| Error::Config(format!("{path} must be an array")))?;
    values
        .iter()
        .enumerate()
        .map(|(index, value)| match value {
            Value::String(raw) => non_empty_string(raw, &format!("{path}[{index}]"))
                .map(ExecPolicyPatternToken::Single),
            Value::Array(alternatives) => {
                let alternatives = alternatives
                    .iter()
                    .enumerate()
                    .map(|(alt_index, value)| {
                        value
                            .as_str()
                            .ok_or_else(|| {
                                Error::Config(format!(
                                    "{path}[{index}][{alt_index}] must be a string"
                                ))
                            })
                            .and_then(|raw| {
                                non_empty_string(raw, &format!("{path}[{index}][{alt_index}]"))
                            })
                    })
                    .collect::<Result<Vec<_>>>()?;
                if alternatives.is_empty() {
                    return Err(Error::Config(format!(
                        "{path}[{index}] alternatives must not be empty"
                    )));
                }
                Ok(ExecPolicyPatternToken::Alternatives(alternatives))
            }
            _ => Err(Error::Config(format!(
                "{path}[{index}] must be a string or array of strings"
            ))),
        })
        .collect()
}

pub(crate) fn exec_policy_examples_field(
    value: Option<&Value>,
    path: &str,
) -> Result<Vec<ExecPolicyExample>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    match value {
        Value::String(_) => Ok(vec![exec_policy_example(value, path)?]),
        Value::Array(values) => {
            if values.iter().all(Value::is_string)
                && (values.len() == 1
                    || values
                        .iter()
                        .filter_map(Value::as_str)
                        .any(|value| value.chars().any(char::is_whitespace)))
            {
                return values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| exec_policy_example(value, &format!("{path}[{index}]")))
                    .collect();
            }
            if values.iter().all(Value::is_string) {
                return Ok(vec![exec_policy_example(value, path)?]);
            }
            values
                .iter()
                .enumerate()
                .map(|(index, value)| exec_policy_example(value, &format!("{path}[{index}]")))
                .collect()
        }
        _ => Err(Error::Config(format!(
            "{path} must be a string, token array, or array of examples"
        ))),
    }
}

pub(crate) fn exec_policy_example(value: &Value, path: &str) -> Result<ExecPolicyExample> {
    match value {
        Value::String(raw) => {
            let raw = non_empty_string(raw, path)?;
            let tokens = crate::permissions::shell_command_tokens(&raw).ok_or_else(|| {
                Error::Config(format!("{path} must be a parseable single shell command"))
            })?;
            Ok(ExecPolicyExample { raw, tokens })
        }
        Value::Array(values) => {
            let tokens = values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    value
                        .as_str()
                        .ok_or_else(|| {
                            Error::Config(format!("{path}[{index}] entries must be strings"))
                        })
                        .and_then(|raw| non_empty_string(raw, &format!("{path}[{index}]")))
                })
                .collect::<Result<Vec<_>>>()?;
            if tokens.is_empty() {
                return Err(Error::Config(format!("{path} must not be empty")));
            }
            Ok(ExecPolicyExample {
                raw: tokens.join(" "),
                tokens,
            })
        }
        _ => Err(Error::Config(format!(
            "{path} must be a string or token array"
        ))),
    }
}

pub(crate) fn validate_exec_policy_rule_examples(
    rule: &ExecPolicyRule,
    index: usize,
    host_executables: &[ExecPolicyHostExecutable],
) -> Result<()> {
    for example in &rule.match_examples {
        if !crate::permissions::exec_prefix_matches(
            &rule.prefix,
            &example.tokens,
            Some(host_executables),
        ) {
            return Err(Error::Config(format!(
                "exec_policy.rules[{index}].match example `{}` does not match prefix",
                example.raw
            )));
        }
    }
    for example in &rule.not_match_examples {
        if crate::permissions::exec_prefix_matches(
            &rule.prefix,
            &example.tokens,
            Some(host_executables),
        ) {
            return Err(Error::Config(format!(
                "exec_policy.rules[{index}].not_match example `{}` matches prefix",
                example.raw
            )));
        }
    }
    Ok(())
}
