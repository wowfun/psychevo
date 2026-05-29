#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn hardline_bash_reason(command: &str) -> Option<String> {
    let compact = command.replace(' ', "");
    if compact.contains("rm-rf/")
        || compact.contains("rm-fr/")
        || compact.contains("rm-rf~")
        || compact.contains("rm-rf$home")
        || compact.contains("rm-rf.")
    {
        return Some("hard-denied recursive delete target".to_string());
    }
    if command.contains("mkfs")
        || command.contains("dd if=") && command.contains(" of=/dev/")
        || command.contains(":(){")
        || command.contains("shutdown")
        || command.contains("reboot")
        || command.contains("poweroff")
        || command.contains("halt")
    {
        return Some("hard-denied system destructive command".to_string());
    }
    None
}

pub(crate) fn dangerous_bash_reason(command: &str) -> Option<String> {
    let dangerous = [
        ("rm -rf", "recursive delete requires approval"),
        ("rm -fr", "recursive delete requires approval"),
        ("chmod -r", "recursive chmod requires approval"),
        ("chown -r", "recursive chown requires approval"),
        (
            "git reset --hard",
            "destructive git reset requires approval",
        ),
        ("git clean -f", "destructive git clean requires approval"),
        ("git push --force", "force push requires approval"),
        ("killall", "process kill requires approval"),
        ("pkill", "process kill requires approval"),
        ("kill -9", "process kill requires approval"),
        ("systemctl", "service management requires approval"),
        ("service ", "service management requires approval"),
        ("sudo ", "sudo command requires approval"),
        ("drop table", "destructive SQL requires approval"),
        ("delete from", "destructive SQL requires approval"),
        ("truncate table", "destructive SQL requires approval"),
        ("find ", "find command requires approval when deleting"),
    ];
    if command.contains("curl") && pipe_to_shell(command)
        || command.contains("wget") && pipe_to_shell(command)
    {
        return Some("downloaded shell installer requires approval".to_string());
    }
    for (needle, reason) in dangerous {
        if command.contains(needle) {
            if needle == "find " && !command.contains("-delete") {
                continue;
            }
            return Some(reason.to_string());
        }
    }
    None
}

pub(crate) fn background_shell_reason(command: &str) -> Option<String> {
    if command.ends_with(" &")
        || command.contains(" & ")
        || command.starts_with("nohup ")
        || command.contains(" nohup ")
        || command.starts_with("disown")
        || command.contains("; disown")
        || command.contains("&& disown")
        || command.starts_with("setsid ")
        || command.contains(" setsid ")
    {
        return Some(
            "shell-level background wrappers are denied; run the foreground command and let exec_command return a session_id"
                .to_string(),
        );
    }
    None
}

pub(crate) fn pipe_to_shell(command: &str) -> bool {
    command.contains("| sh")
        || command.contains("| bash")
        || command.contains("|sh")
        || command.contains("|bash")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InlineInterpreterReview {
    LiteralFileReads(Vec<String>),
    NeedsApproval(String),
}

pub(crate) fn is_known_safe_command(command: &[String]) -> bool {
    if is_safe_to_call_with_exec(command) {
        return true;
    }
    shell_lc_word_only_commands(command).is_some_and(|commands| {
        !commands.is_empty()
            && commands
                .iter()
                .all(|command| is_safe_to_call_with_exec(command))
    })
}

pub(crate) fn is_safe_to_call_with_exec(command: &[String]) -> bool {
    let Some(cmd) = command.first().and_then(|raw| shell_basename(raw)) else {
        return false;
    };
    match cmd.as_str() {
        "cat" | "cd" | "cut" | "echo" | "false" | "grep" | "head" | "ls" | "pwd" | "tail"
        | "true" | "wc" | "which" | "whoami" => true,
        "rg" => safe_rg_args(&command[1..]),
        "sed" => safe_sed_args(&command[1..]),
        "git" => safe_git_command(command),
        _ => false,
    }
}

pub(crate) fn inline_interpreter_review(
    raw_command: &str,
    command: &[String],
) -> Option<InlineInterpreterReview> {
    if let Some(review) = inline_interpreter_tokens_review(command) {
        return Some(review);
    }
    if let Some(commands) = shell_lc_word_only_commands(command) {
        let mut paths = Vec::new();
        for command in commands {
            match inline_interpreter_tokens_review(&command) {
                Some(InlineInterpreterReview::LiteralFileReads(mut next)) => {
                    paths.append(&mut next);
                }
                Some(InlineInterpreterReview::NeedsApproval(reason)) => {
                    return Some(InlineInterpreterReview::NeedsApproval(reason));
                }
                None => {}
            }
        }
        if !paths.is_empty() {
            paths.sort();
            paths.dedup();
            return Some(InlineInterpreterReview::LiteralFileReads(paths));
        }
    }
    if contains_inline_interpreter(raw_command) {
        return Some(InlineInterpreterReview::NeedsApproval(
            "inline interpreter execution requires approval".to_string(),
        ));
    }
    None
}

pub(crate) fn is_inline_interpreter_tokens(command: &[String]) -> bool {
    inline_interpreter_script(command).is_some()
}

pub(crate) fn inline_interpreter_tokens_review(
    command: &[String],
) -> Option<InlineInterpreterReview> {
    let (interpreter, script) = inline_interpreter_script(command)?;
    if matches!(interpreter.as_str(), "python" | "python3") {
        return Some(match literal_python_file_reads(script) {
            Ok(paths) => InlineInterpreterReview::LiteralFileReads(paths),
            Err(reason) => InlineInterpreterReview::NeedsApproval(reason),
        });
    }
    Some(InlineInterpreterReview::NeedsApproval(
        "inline interpreter execution requires approval".to_string(),
    ))
}

pub(crate) fn inline_interpreter_script(command: &[String]) -> Option<(String, &str)> {
    let interpreter = command.first().and_then(|raw| shell_basename(raw))?;
    let flag = command.get(1).map(String::as_str)?;
    let script = command.get(2).map(String::as_str)?;
    match (interpreter.as_str(), flag) {
        ("python" | "python3", "-c") | ("node", "-e") | ("perl", "-e") | ("ruby", "-e") => {
            Some((interpreter, script))
        }
        _ => None,
    }
}

pub(crate) fn contains_inline_interpreter(command: &str) -> bool {
    ["python -c", "python3 -c", "node -e", "perl -e", "ruby -e"]
        .iter()
        .any(|needle| command.contains(needle))
}

pub(crate) fn literal_python_file_reads(script: &str) -> std::result::Result<Vec<String>, String> {
    let lowered = script.to_ascii_lowercase();
    for risky in [
        "subprocess",
        "os.system",
        "socket",
        "requests",
        "urllib",
        "http.client",
        "eval(",
        "exec(",
        "__import__",
        ".write(",
        "write_text",
        "write_bytes",
        "remove(",
        "unlink(",
        "rmtree",
        "rename(",
        "replace(",
        "chmod(",
        "chown(",
        "mkdir(",
        "makedirs(",
    ] {
        if lowered.contains(risky) {
            return Err(format!(
                "inline Python contains `{risky}` and requires approval"
            ));
        }
    }
    let mut paths = literal_open_read_paths(script)?;
    paths.extend(literal_pathlib_read_paths(script)?);
    paths.sort();
    paths.dedup();
    if paths.is_empty() {
        return Err(
            "inline Python could not be statically reduced to literal file reads".to_string(),
        );
    }
    Ok(paths)
}

pub(crate) fn literal_open_read_paths(script: &str) -> std::result::Result<Vec<String>, String> {
    let mut paths = Vec::new();
    let mut offset = 0usize;
    while let Some(found) = script[offset..].find("open(") {
        let start = offset + found + "open(".len();
        let Some((path, after_path)) = parse_literal_string_at(script, start) else {
            return Err("inline Python open() path is not a literal string".to_string());
        };
        if python_open_mode_is_mutating(script, after_path) {
            return Err("inline Python open() uses a mutating mode".to_string());
        }
        paths.push(path);
        offset = after_path;
    }
    Ok(paths)
}

pub(crate) fn literal_pathlib_read_paths(script: &str) -> std::result::Result<Vec<String>, String> {
    let mut paths = Vec::new();
    let mut offset = 0usize;
    while let Some(found) = script[offset..].find("Path(") {
        let start = offset + found + "Path(".len();
        let Some((path, after_path)) = parse_literal_string_at(script, start) else {
            return Err("inline Python Path() argument is not a literal string".to_string());
        };
        let rest = &script[after_path..script.len().min(after_path + 80)];
        if rest.contains(".read_text(") || rest.contains(".read_bytes(") {
            paths.push(path);
        } else {
            return Err("inline Python Path() is not a recognized read".to_string());
        }
        offset = after_path;
    }
    Ok(paths)
}

pub(crate) fn parse_literal_string_at(script: &str, start: usize) -> Option<(String, usize)> {
    let bytes = script.as_bytes();
    let mut index = start;
    while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
        index += 1;
    }
    let quote = *bytes.get(index)?;
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    index += 1;
    let value_start = index;
    let mut escaped = false;
    while let Some(byte) = bytes.get(index).copied() {
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == quote {
            return Some((script[value_start..index].to_string(), index + 1));
        }
        index += 1;
    }
    None
}

pub(crate) fn python_open_mode_is_mutating(script: &str, after_path: usize) -> bool {
    let end = script[after_path..]
        .find(')')
        .map(|index| after_path + index)
        .unwrap_or(script.len());
    let args = script[after_path..end].to_ascii_lowercase();
    [
        "'w'", "\"w\"", "'a'", "\"a\"", "'x'", "\"x\"", "'w+", "\"w+", "'a+", "\"a+", "'x+", "\"x+",
    ]
    .iter()
    .any(|needle| args.contains(needle))
        || args.contains("mode='w")
        || args.contains("mode=\"w")
        || args.contains("mode='a")
        || args.contains("mode=\"a")
        || args.contains("mode='x")
        || args.contains("mode=\"x")
        || args.contains('+')
}

pub(crate) fn safe_rg_args(args: &[String]) -> bool {
    !args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--pre" | "--hostname-bin" | "--search-zip" | "-z"
        ) || arg.starts_with("--pre=")
            || arg.starts_with("--hostname-bin=")
    })
}

pub(crate) fn safe_sed_args(args: &[String]) -> bool {
    args.len() <= 3
        && args.first().map(String::as_str) == Some("-n")
        && args
            .get(1)
            .map(String::as_str)
            .is_some_and(is_valid_sed_n_arg)
}

pub(crate) fn safe_git_command(command: &[String]) -> bool {
    let Some((index, subcommand)) = git_subcommand(command) else {
        return false;
    };
    if git_has_unsafe_global_option(&command[1..index]) {
        return false;
    }
    let args = &command[index + 1..];
    match subcommand {
        "status" | "log" | "diff" | "show" => git_args_are_read_only(args),
        "branch" => git_args_are_read_only(args) && git_branch_is_read_only(args),
        _ => false,
    }
}

pub(crate) fn git_subcommand(command: &[String]) -> Option<(usize, &str)> {
    if command
        .first()
        .and_then(|raw| shell_basename(raw))
        .as_deref()
        != Some("git")
    {
        return None;
    }
    let mut skip_next = false;
    for (index, arg) in command.iter().enumerate().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }
        let arg = arg.as_str();
        if matches!(
            arg,
            "-C" | "-c"
                | "--config-env"
                | "--exec-path"
                | "--git-dir"
                | "--namespace"
                | "--super-prefix"
                | "--work-tree"
        ) {
            skip_next = true;
            continue;
        }
        if arg.starts_with("-C")
            || arg.starts_with("-c")
            || arg.starts_with("--config-env=")
            || arg.starts_with("--exec-path=")
            || arg.starts_with("--git-dir=")
            || arg.starts_with("--namespace=")
            || arg.starts_with("--super-prefix=")
            || arg.starts_with("--work-tree=")
        {
            continue;
        }
        if arg == "--" || arg.starts_with('-') {
            continue;
        }
        return matches!(arg, "status" | "log" | "diff" | "show" | "branch")
            .then_some((index, arg));
    }
    None
}

pub(crate) fn git_has_unsafe_global_option(args: &[String]) -> bool {
    args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "-C" | "-c"
                | "-p"
                | "--config-env"
                | "--exec-path"
                | "--git-dir"
                | "--namespace"
                | "--paginate"
                | "--super-prefix"
                | "--work-tree"
        ) || arg.starts_with("-C")
            || arg.starts_with("-c")
            || arg.starts_with("--config-env=")
            || arg.starts_with("--exec-path=")
            || arg.starts_with("--git-dir=")
            || arg.starts_with("--namespace=")
            || arg.starts_with("--super-prefix=")
            || arg.starts_with("--work-tree=")
    })
}

pub(crate) fn git_args_are_read_only(args: &[String]) -> bool {
    !args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--output" | "--ext-diff" | "--textconv" | "--exec"
        ) || arg.starts_with("--output=")
            || arg.starts_with("--exec=")
    })
}

pub(crate) fn git_branch_is_read_only(args: &[String]) -> bool {
    if args.is_empty() {
        return true;
    }
    let mut saw_read_only = false;
    for arg in args {
        match arg.as_str() {
            "--list" | "-l" | "--show-current" | "-a" | "--all" | "-r" | "--remotes" | "-v"
            | "-vv" | "--verbose" => saw_read_only = true,
            raw if raw.starts_with("--format=") => saw_read_only = true,
            _ => return false,
        }
    }
    saw_read_only
}

pub(crate) fn is_valid_sed_n_arg(value: &str) -> bool {
    let Some(core) = value.strip_suffix('p') else {
        return false;
    };
    let parts = core.split(',').collect::<Vec<_>>();
    match parts.as_slice() {
        [single] => !single.is_empty() && single.chars().all(|ch| ch.is_ascii_digit()),
        [start, end] => {
            !start.is_empty()
                && !end.is_empty()
                && start.chars().all(|ch| ch.is_ascii_digit())
                && end.chars().all(|ch| ch.is_ascii_digit())
        }
        _ => false,
    }
}

pub(crate) fn action_summary(tool_name: &str, args: &Value) -> String {
    match tool_name {
        "exec_command" => args
            .get("cmd")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        "read" | "write" | "edit" => args
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("(patch)")
            .to_string(),
        "web_fetch" => args
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        "mcp_startup" => args
            .get("server")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        _ if tool_name.starts_with("mcp__") => crate::mcp::mcp_tool_name_parts(tool_name)
            .map(|(server, tool)| format!("{server}/{tool}"))
            .unwrap_or_else(|| args.to_string()),
        _ => args.to_string(),
    }
}

pub(crate) fn permission_error(
    decision: &str,
    reason: &str,
    matched_rule: Option<&str>,
) -> ToolOutput {
    ToolOutput {
        json: json!({
            "error": format!("permission {decision}: {reason}"),
            "permission": {
                "decision": decision,
                "reason": reason,
                "matched_rule": matched_rule,
            }
        }),
        model_content: None,
        attachments: Vec::new(),
        is_error: true,
    }
}

#[allow(dead_code)]
pub(crate) fn parse_rules(rules: Vec<String>) -> Vec<PermissionRule> {
    rules
        .into_iter()
        .filter_map(|raw| parse_rule(&raw))
        .collect()
}

#[allow(dead_code)]
pub(crate) fn parse_rule(raw: &str) -> Option<PermissionRule> {
    let raw = raw.trim();
    let (tool, rest) = raw.split_once('(')?;
    let pattern = rest.strip_suffix(')')?.trim();
    let tool = match tool.trim() {
        "ExecCommand" | "exec_command" => "exec_command",
        "Read" | "read" => "read",
        "Write" | "write" => "write",
        "Edit" | "edit" => "edit",
        "SkillManage" | "skill_manage" => "skill_manage",
        "SkillHub" | "skill_hub" => "skill_hub",
        "SkillConfig" | "skill_config" => "skill_config",
        "McpStartup" | "mcp_startup" => "mcp_startup",
        "Mcp" | "mcp" => "mcp",
        "WebFetch" | "web_fetch" => "web_fetch",
        _ => return None,
    };
    Some(PermissionRule {
        raw: raw.to_string(),
        tool: tool.to_string(),
        pattern: normalize_rule_pattern(pattern, tool),
    })
}

#[allow(dead_code)]
pub(crate) fn normalize_rule_pattern(pattern: &str, tool: &str) -> String {
    if tool == "exec_command" {
        normalize_command(pattern)
    } else {
        pattern.replace('\\', "/")
    }
}

pub(crate) fn normalize_command(command: &str) -> String {
    command
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

pub(crate) fn wildcard_match(pattern: &str, text: &str) -> bool {
    let pattern = pattern.as_bytes();
    let text = text.as_bytes();
    let (mut p, mut t) = (0usize, 0usize);
    let mut star = None;
    let mut match_after_star = 0usize;
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            match_after_star = t;
            p += 1;
        } else if let Some(star_index) = star {
            p = star_index + 1;
            match_after_star += 1;
            t = match_after_star;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::*;
    use std::collections::BTreeMap;

    use crate::types::{
        ApprovalHandler, ExecPolicyConfig, ExecPolicyRule, PermissionApprovalDecision,
    };

    fn runtime(config: PermissionConfig, mode: PermissionMode) -> PermissionRuntime {
        PermissionRuntime::new(
            PathBuf::from("/repo"),
            PathBuf::from("/repo/.psychevo"),
            config,
            mode,
            ApprovalMode::Manual,
            None,
            None,
        )
    }

    fn exec_rule(prefix: &[&str], decision: ExecPolicyDecision) -> ExecPolicyRule {
        ExecPolicyRule {
            prefix: prefix
                .iter()
                .map(|value| ExecPolicyPatternToken::Single((*value).to_string()))
                .collect(),
            decision,
            justification: None,
            match_examples: Vec::new(),
            not_match_examples: Vec::new(),
        }
    }

    #[derive(Debug)]
    struct PendingApprovalHandler;

    impl ApprovalHandler for PendingApprovalHandler {
        fn timeout_secs(&self) -> u64 {
            0
        }

        fn request_permission(
            &self,
            _request: PermissionApprovalRequest,
        ) -> BoxFuture<'static, PermissionApprovalDecision> {
            Box::pin(std::future::pending())
        }
    }

    #[test]
    fn hardline_denies_win_over_allow() {
        let runtime = runtime(
            PermissionConfig::default(),
            PermissionMode::BypassPermissions,
        );
        let decision = runtime.evaluate("exec_command", &json!({"cmd": "rm -rf /"}));
        assert!(matches!(decision, PermissionDecision::Deny { .. }));
    }

    #[test]
    fn configured_precedence_is_deny_ask_allow() {
        let runtime = runtime(
            PermissionConfig {
                exec_policy: ExecPolicyConfig {
                    rules: vec![
                        exec_rule(&["cargo", "publish"], ExecPolicyDecision::Allow),
                        exec_rule(&["cargo", "publish"], ExecPolicyDecision::Prompt),
                        exec_rule(&["cargo", "publish"], ExecPolicyDecision::Deny),
                    ],
                    ..Default::default()
                },
                ..Default::default()
            },
            PermissionMode::Default,
        );
        let decision = runtime.evaluate("exec_command", &json!({"cmd": "cargo publish --dry-run"}));
        assert!(matches!(decision, PermissionDecision::Deny { .. }));
    }

    #[test]
    fn accept_edits_allows_safe_file_asks() {
        let mut profiles = BTreeMap::new();
        profiles.insert(
            "local".to_string(),
            PermissionProfileConfig {
                extends: Some(":workspace".to_string()),
                filesystem: BTreeMap::from([("src".to_string(), PermissionAccess::Prompt)]),
                ..Default::default()
            },
        );
        let runtime = runtime(
            PermissionConfig {
                default_permissions: "local".to_string(),
                profiles,
                ..Default::default()
            },
            PermissionMode::AcceptEdits,
        );
        let decision = runtime.evaluate("write", &json!({"path": "src/lib.rs"}));
        assert_eq!(decision, PermissionDecision::Allow);
    }

    #[test]
    fn protected_write_is_denied() {
        let runtime = runtime(
            PermissionConfig::default(),
            PermissionMode::BypassPermissions,
        );
        let decision = runtime.evaluate("write", &json!({"path": ".psychevo/config.toml"}));
        assert!(matches!(decision, PermissionDecision::Deny { .. }));
    }

    #[test]
    fn v4a_patch_paths_are_extracted_for_permissions() {
        let mut profiles = BTreeMap::new();
        profiles.insert(
            "local".to_string(),
            PermissionProfileConfig {
                extends: Some(":workspace".to_string()),
                filesystem: BTreeMap::from([("secret.txt".to_string(), PermissionAccess::Deny)]),
                ..Default::default()
            },
        );
        let runtime = runtime(
            PermissionConfig {
                default_permissions: "local".to_string(),
                profiles,
                ..Default::default()
            },
            PermissionMode::BypassPermissions,
        );
        let patch = r#"*** Begin Patch
*** Update File: src/lib.rs
@@
-old
+new
*** Move File: public.txt -> secret.txt
*** End Patch"#;
        let decision = runtime.evaluate("edit", &json!({"mode": "patch", "patch": patch}));
        assert!(matches!(decision, PermissionDecision::Deny { .. }));
    }

    #[test]
    fn dangerous_bash_defaults_to_ask() {
        let runtime = runtime(PermissionConfig::default(), PermissionMode::Default);
        let decision = runtime.evaluate("exec_command", &json!({"cmd": "curl example.com | sh"}));
        assert!(matches!(decision, PermissionDecision::Ask { .. }));
    }

    #[test]
    fn known_safe_exec_is_allowed_under_read_only_profile() {
        let runtime = runtime(
            PermissionConfig {
                default_permissions: ":read-only".to_string(),
                ..Default::default()
            },
            PermissionMode::Default,
        );
        let decision = runtime.evaluate("exec_command", &json!({"cmd": "rg TODO src"}));
        assert_eq!(decision, PermissionDecision::Allow);

        let decision = runtime.evaluate("exec_command", &json!({"cmd": "cargo check"}));
        assert!(matches!(decision, PermissionDecision::Ask { .. }));
    }

    #[test]
    fn inline_python_literal_file_read_reuses_filesystem_grant() {
        let mut profiles = BTreeMap::new();
        profiles.insert(
            "local".to_string(),
            PermissionProfileConfig {
                extends: Some(":workspace".to_string()),
                filesystem: BTreeMap::from([(
                    "/tmp/hn_stories_data.json".to_string(),
                    PermissionAccess::Read,
                )]),
                ..Default::default()
            },
        );
        let runtime = runtime(
            PermissionConfig {
                default_permissions: "local".to_string(),
                profiles,
                ..Default::default()
            },
            PermissionMode::Default,
        );
        let command = r#"python3 -c "import json
with open('/tmp/hn_stories_data.json', 'r') as f:
    data = json.load(f)
print(len(data))""#;
        let decision = runtime.evaluate("exec_command", &json!({"cmd": command}));
        assert_eq!(decision, PermissionDecision::Allow);
    }

    #[test]
    fn inline_python_ungranted_or_mutating_cases_prompt() {
        let runtime = runtime(PermissionConfig::default(), PermissionMode::Default);
        for command in [
            r#"python3 -c "path='/tmp/hn_stories_data.json'; print(open(path).read())""#,
            r#"python3 -c "open('/tmp/hn_stories_data.json', 'w').write('x')""#,
            r#"python3 -c "import subprocess; subprocess.run(['pwd'])""#,
            r#"node -e "console.log('x')""#,
        ] {
            let decision = runtime.evaluate("exec_command", &json!({"cmd": command}));
            assert!(
                matches!(decision, PermissionDecision::Ask { .. }),
                "{command}"
            );
        }
    }

    #[test]
    fn legacy_bash_rules_do_not_match_exec_command() {
        let runtime = runtime(
            PermissionConfig {
                exec_policy: ExecPolicyConfig {
                    rules: vec![exec_rule(&["Bash", "cargo"], ExecPolicyDecision::Deny)],
                    ..Default::default()
                },
                ..Default::default()
            },
            PermissionMode::Default,
        );
        let decision = runtime.evaluate("exec_command", &json!({"cmd": "cargo publish --dry-run"}));
        assert!(!matches!(decision, PermissionDecision::Deny { .. }));
    }

    #[test]
    fn exec_policy_host_executable_path_controls_basename_fallback() {
        let rule = exec_rule(&["git", "status"], ExecPolicyDecision::Deny);
        let runtime = runtime(
            PermissionConfig {
                exec_policy: ExecPolicyConfig {
                    rules: vec![rule],
                    host_executables: vec![crate::types::ExecPolicyHostExecutable {
                        name: "git".to_string(),
                        paths: vec!["/usr/bin/git".to_string()],
                    }],
                },
                ..Default::default()
            },
            PermissionMode::Default,
        );
        let decision = runtime.evaluate("exec_command", &json!({"cmd": "/usr/bin/git status"}));
        assert!(matches!(decision, PermissionDecision::Deny { .. }));

        let decision = runtime.evaluate("exec_command", &json!({"cmd": "/tmp/git status"}));
        assert!(!matches!(decision, PermissionDecision::Deny { .. }));
    }

    #[test]
    fn shell_background_wrappers_are_hard_denied() {
        let runtime = runtime(
            PermissionConfig::default(),
            PermissionMode::BypassPermissions,
        );
        let decision = runtime.evaluate("exec_command", &json!({"cmd": "sleep 60 &"}));
        assert!(matches!(decision, PermissionDecision::Deny { .. }));
    }

    #[test]
    fn outside_workdir_exec_workdir_defaults_to_ask() {
        let runtime = runtime(PermissionConfig::default(), PermissionMode::Default);
        let decision = runtime.evaluate("exec_command", &json!({"cmd": "pwd", "workdir": "/tmp"}));
        assert!(matches!(decision, PermissionDecision::Ask { .. }));
    }

    #[test]
    fn web_fetch_defaults_to_allow_but_profile_rules_match_hosts() {
        let default_runtime = runtime(PermissionConfig::default(), PermissionMode::Default);
        let decision =
            default_runtime.evaluate("web_fetch", &json!({"url": "https://example.com/a"}));
        assert_eq!(decision, PermissionDecision::Allow);

        let mut profiles = BTreeMap::new();
        profiles.insert(
            "local".to_string(),
            PermissionProfileConfig {
                extends: Some(":workspace".to_string()),
                network_domains: BTreeMap::from([(
                    "example.com".to_string(),
                    PermissionAccess::Prompt,
                )]),
                ..Default::default()
            },
        );
        let prompt_runtime = runtime(
            PermissionConfig {
                default_permissions: "local".to_string(),
                profiles,
                ..Default::default()
            },
            PermissionMode::Default,
        );
        let decision =
            prompt_runtime.evaluate("web_fetch", &json!({"url": "https://example.com/a"}));
        assert!(matches!(decision, PermissionDecision::Ask { .. }));

        let mut profiles = BTreeMap::new();
        profiles.insert(
            "local".to_string(),
            PermissionProfileConfig {
                extends: Some(":workspace".to_string()),
                network_domains: BTreeMap::from([(
                    "example.com".to_string(),
                    PermissionAccess::Deny,
                )]),
                ..Default::default()
            },
        );
        let deny_runtime = runtime(
            PermissionConfig {
                default_permissions: "local".to_string(),
                profiles,
                ..Default::default()
            },
            PermissionMode::Default,
        );
        let decision = deny_runtime.evaluate("web_fetch", &json!({"url": "https://example.com/a"}));
        assert!(matches!(decision, PermissionDecision::Deny { .. }));
    }

    #[tokio::test]
    async fn web_fetch_authorizes_without_approval_handler_by_default() {
        let runtime = runtime(PermissionConfig::default(), PermissionMode::Default);
        runtime
            .authorize(
                "call-1",
                "web_fetch",
                &json!({"url": "https://example.com/a"}),
            )
            .await
            .expect("default web_fetch should not need approval");
    }

    #[test]
    fn mcp_tools_default_to_ask_and_match_rules() {
        let default_runtime = runtime(PermissionConfig::default(), PermissionMode::Default);
        let decision = default_runtime.evaluate("mcp__repo_tools__read_file", &json!({}));
        assert!(matches!(decision, PermissionDecision::Ask { .. }));
    }

    #[test]
    fn mcp_startup_defaults_to_ask_and_matches_rules() {
        let args = json!({"server": "repo_tools", "transport": "stdio"});
        let default_runtime = runtime(PermissionConfig::default(), PermissionMode::Default);
        let decision = default_runtime.evaluate("mcp_startup", &args);
        assert!(matches!(decision, PermissionDecision::Ask { .. }));
    }

    #[tokio::test]
    async fn dont_ask_denies_actions_that_would_prompt() {
        let runtime = runtime(PermissionConfig::default(), PermissionMode::DontAsk);
        let output = runtime
            .authorize(
                "call-1",
                "exec_command",
                &json!({"cmd": "curl example.com | sh"}),
            )
            .await
            .expect_err("dontAsk should deny explicit ask rules");
        assert!(output.is_error);
        assert_eq!(output.json["permission"]["decision"], "denied");
        assert!(
            output.json["permission"]["reason"]
                .as_str()
                .unwrap_or_default()
                .contains("dontAsk")
        );
    }

    #[tokio::test]
    async fn missing_approval_handler_fails_closed() {
        let runtime = runtime(PermissionConfig::default(), PermissionMode::Default);
        let output = runtime
            .authorize(
                "call-1",
                "read",
                &json!({"path": "/tmp/outside-workdir.txt"}),
            )
            .await
            .expect_err("outside workdir read should need a handler");
        assert!(output.is_error);
        assert_eq!(output.json["permission"]["decision"], "denied");
        assert!(
            output.json["permission"]["reason"]
                .as_str()
                .unwrap_or_default()
                .contains("failing closed")
        );
    }

    #[tokio::test]
    async fn never_policy_denies_without_prompt() {
        let runtime = runtime(
            PermissionConfig {
                approval_policy: ApprovalPolicy::Never,
                ..Default::default()
            },
            PermissionMode::Default,
        );
        let output = runtime
            .authorize(
                "call-1",
                "exec_command",
                &json!({"cmd": "curl example.com | sh"}),
            )
            .await
            .expect_err("never should deny prompts");
        assert!(output.is_error);
        assert!(
            output.json["permission"]["reason"]
                .as_str()
                .unwrap_or_default()
                .contains("approval_policy=never")
        );
    }

    #[tokio::test]
    async fn authorization_prompt_wakes_when_abort_signal_trips() {
        let runtime = PermissionRuntime::new(
            PathBuf::from("/repo"),
            PathBuf::from("/repo/.psychevo"),
            PermissionConfig::default(),
            PermissionMode::Default,
            ApprovalMode::Manual,
            Some(Arc::new(PendingApprovalHandler)),
            None,
        );
        let runtime_for_task = runtime.clone();
        let (abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(async move {
            runtime_for_task
                .authorize_with_abort(
                    "call-1",
                    "exec_command",
                    &json!({"cmd": "curl example.com | sh"}),
                    AbortSignal::new(abort_rx),
                )
                .await
        });

        tokio::task::yield_now().await;
        abort_tx.send(true).expect("abort");
        let output = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("authorization should wake")
            .expect("task should not panic")
            .expect_err("authorization should abort");

        assert!(output.is_error);
        assert_eq!(output.json["error"], "aborted");
        assert_eq!(
            runtime.approval_lifecycle_events(),
            vec![
                ApprovalLifecycleEvent::Requested {
                    tool_call_id: "call-1".to_string(),
                    tool_name: "exec_command".to_string(),
                },
                ApprovalLifecycleEvent::Aborted {
                    tool_call_id: "call-1".to_string(),
                },
            ]
        );
    }

    #[test]
    fn profile_deny_wins_over_session_grant() {
        let mut profiles = BTreeMap::new();
        profiles.insert(
            "local".to_string(),
            PermissionProfileConfig {
                extends: Some(":workspace".to_string()),
                filesystem: BTreeMap::from([("secret.txt".to_string(), PermissionAccess::Deny)]),
                ..Default::default()
            },
        );
        let runtime = runtime(
            PermissionConfig {
                default_permissions: "local".to_string(),
                profiles,
                ..Default::default()
            },
            PermissionMode::Default,
        );
        runtime.remember_session_grant("read:secret.txt".to_string());
        let decision = runtime.evaluate("read", &json!({"path": "secret.txt"}));
        assert!(matches!(decision, PermissionDecision::Deny { .. }));
    }
}
