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
        || contains_system_destructive_command(command)
    {
        return Some("hard-denied system destructive command".to_string());
    }
    None
}

pub(crate) fn contains_system_destructive_command(command: &str) -> bool {
    shell_command_invocations(command).is_some_and(|commands| {
        commands
            .iter()
            .any(|command| is_system_destructive_invocation(command))
    })
}

pub(crate) fn is_system_destructive_invocation(command: &[String]) -> bool {
    let Some((name, args)) = effective_system_command(command) else {
        return false;
    };
    match name.as_str() {
        "shutdown" | "reboot" | "poweroff" | "halt" => true,
        "systemctl" => systemctl_destructive_action(args).is_some(),
        _ => false,
    }
}

fn effective_system_command(command: &[String]) -> Option<(String, &[String])> {
    let mut index = 0usize;
    while index < command.len() {
        let name = shell_basename(command.get(index)?)?;
        match name.as_str() {
            "sudo" => {
                index += 1;
                while let Some(arg) = command.get(index) {
                    if arg == "--" {
                        index += 1;
                        break;
                    }
                    if !arg.starts_with('-') || arg == "-" {
                        break;
                    }
                    let takes_value = matches!(
                        arg.as_str(),
                        "-A" | "-C"
                            | "-D"
                            | "-g"
                            | "-h"
                            | "-p"
                            | "-R"
                            | "-r"
                            | "-T"
                            | "-t"
                            | "-U"
                            | "-u"
                    );
                    index += 1;
                    if takes_value && command.get(index).is_some() {
                        index += 1;
                    }
                }
            }
            "env" => {
                index += 1;
                while let Some(arg) = command.get(index) {
                    if arg == "--" {
                        index += 1;
                        break;
                    }
                    if arg == "-u" || arg == "--unset" || arg == "-S" || arg == "--split-string" {
                        index += 2;
                        continue;
                    }
                    if arg.starts_with('-') || is_env_assignment(arg) {
                        index += 1;
                        continue;
                    }
                    break;
                }
            }
            "exec" | "nohup" | "setsid" => {
                index += 1;
            }
            _ => {
                return Some((name, &command[index + 1..]));
            }
        }
    }
    None
}

fn is_env_assignment(arg: &str) -> bool {
    let Some((name, _value)) = arg.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        && name
            .chars()
            .next()
            .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
}

fn systemctl_destructive_action(args: &[String]) -> Option<&str> {
    args.iter()
        .find(|arg| !arg.starts_with('-') || arg.as_str() == "-")
        .map(String::as_str)
        .filter(|action| matches!(*action, "poweroff" | "reboot" | "halt" | "kexec"))
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
