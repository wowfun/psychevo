pub(crate) fn command_tokens(command: &str) -> Vec<String> {
    shell_command_tokens(command).unwrap_or_default()
}

pub(crate) fn exec_prefix_matches(
    prefix: &[ExecPolicyPatternToken],
    tokens: &[String],
    host_executables: Option<&[crate::types::ExecPolicyHostExecutable]>,
) -> bool {
    if prefix.len() > tokens.len() || prefix.is_empty() {
        return false;
    }
    if prefix
        .iter()
        .zip(tokens)
        .all(|(pattern, token)| pattern.matches(token))
    {
        return true;
    }
    let Some(host_executables) = host_executables else {
        return false;
    };
    let Some(first_token) = tokens.first() else {
        return false;
    };
    if !Path::new(first_token).is_absolute() {
        return false;
    }
    let Some(basename) = shell_basename(first_token) else {
        return false;
    };
    let first_pattern = &prefix[0];
    if !first_pattern.matches(&basename) {
        return false;
    }
    if !host_executable_allows_path(host_executables, &basename, first_token) {
        return false;
    }
    prefix
        .iter()
        .skip(1)
        .zip(tokens.iter().skip(1))
        .all(|(pattern, token)| pattern.matches(token))
}

pub(crate) fn exec_prefix_label(prefix: &[ExecPolicyPatternToken]) -> String {
    prefix
        .iter()
        .map(|token| match token {
            ExecPolicyPatternToken::Single(value) => value.clone(),
            ExecPolicyPatternToken::Alternatives(values) => format!("[{}]", values.join("|")),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn host_executable_allows_path(
    host_executables: &[crate::types::ExecPolicyHostExecutable],
    name: &str,
    path: &str,
) -> bool {
    match host_executables.iter().find(|host| host.name == name) {
        Some(host) => host.paths.iter().any(|allowed| allowed == path),
        None => true,
    }
}

pub(crate) fn exec_grant_prefix(command: &str) -> Option<Vec<String>> {
    let direct = command_tokens(command);
    if direct.is_empty() {
        return None;
    }
    if let Some(commands) = shell_lc_word_only_commands(&direct) {
        return commands
            .into_iter()
            .find(|command| !is_known_safe_command(command))
            .and_then(|command| risky_command_prefix(&command));
    }
    risky_command_prefix(&direct)
}

pub(crate) fn risky_command_prefix(command: &[String]) -> Option<Vec<String>> {
    if command.is_empty() {
        return None;
    }
    if is_inline_interpreter_tokens(command) {
        return Some(command.iter().take(2).cloned().collect());
    }
    if shell_basename(&command[0]).as_deref() == Some("git")
        && let Some((_index, subcommand)) = git_subcommand(command)
    {
        return Some(vec![command[0].clone(), subcommand.to_string()]);
    }
    Some(command.iter().take(command.len().min(2)).cloned().collect())
}

pub(crate) fn permission_rule_tool(tool: &str) -> &str {
    match tool {
        "skill_manage" => "SkillManage",
        "skill_hub" => "SkillHub",
        "skill_config" => "SkillConfig",
        "mcp_startup" => "McpStartup",
        "mcp" => "Mcp",
        "web_fetch" => "WebFetch",
        "web_search" => "WebSearch",
        other => other,
    }
}
