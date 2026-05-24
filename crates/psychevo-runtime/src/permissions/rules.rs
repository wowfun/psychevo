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
    if command.contains("python -c")
        || command.contains("python3 -c")
        || command.contains("node -e")
        || command.contains("perl -e")
        || command.contains("ruby -e")
    {
        return Some("inline interpreter execution requires approval".to_string());
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

pub(crate) fn parse_rules(rules: Vec<String>) -> Vec<PermissionRule> {
    rules
        .into_iter()
        .filter_map(|raw| parse_rule(&raw))
        .collect()
}

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

    fn runtime(config: PermissionConfig, mode: PermissionMode) -> PermissionRuntime {
        PermissionRuntime::new(
            PathBuf::from("/repo"),
            PathBuf::from("/repo/.psychevo"),
            config,
            mode,
            ApprovalMode::Manual,
            None,
        )
    }

    #[test]
    fn hardline_denies_win_over_allow() {
        let runtime = runtime(
            PermissionConfig {
                allow: vec!["ExecCommand(rm -rf /)".to_string()],
                ..Default::default()
            },
            PermissionMode::BypassPermissions,
        );
        let decision = runtime.evaluate("exec_command", &json!({"cmd": "rm -rf /"}));
        assert!(matches!(decision, PermissionDecision::Deny { .. }));
    }

    #[test]
    fn configured_precedence_is_deny_ask_allow() {
        let runtime = runtime(
            PermissionConfig {
                allow: vec!["ExecCommand(cargo publish *)".to_string()],
                ask: vec!["ExecCommand(cargo publish *)".to_string()],
                deny: vec!["ExecCommand(cargo publish *)".to_string()],
                ..Default::default()
            },
            PermissionMode::Default,
        );
        let decision = runtime.evaluate("exec_command", &json!({"cmd": "cargo publish --dry-run"}));
        assert!(matches!(decision, PermissionDecision::Deny { .. }));
    }

    #[test]
    fn accept_edits_allows_safe_file_asks() {
        let runtime = runtime(
            PermissionConfig {
                ask: vec!["Write(src/*)".to_string()],
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
        let runtime = runtime(
            PermissionConfig {
                deny: vec!["Edit(secret.txt)".to_string()],
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
    fn legacy_bash_rules_do_not_match_exec_command() {
        let runtime = runtime(
            PermissionConfig {
                deny: vec!["Bash(cargo publish *)".to_string()],
                ..Default::default()
            },
            PermissionMode::Default,
        );
        let decision = runtime.evaluate("exec_command", &json!({"cmd": "cargo publish --dry-run"}));
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
    fn web_fetch_defaults_to_allow_but_rules_match_urls() {
        let default_runtime = runtime(PermissionConfig::default(), PermissionMode::Default);
        let decision =
            default_runtime.evaluate("web_fetch", &json!({"url": "https://example.com/a"}));
        assert_eq!(decision, PermissionDecision::Allow);

        let deny_runtime = runtime(
            PermissionConfig {
                deny: vec!["WebFetch(https://example.com/*)".to_string()],
                ..Default::default()
            },
            PermissionMode::Default,
        );
        let decision = deny_runtime.evaluate("web_fetch", &json!({"url": "https://example.com/a"}));
        assert!(matches!(decision, PermissionDecision::Deny { .. }));
    }

    #[test]
    fn mcp_tools_default_to_ask_and_match_rules() {
        let default_runtime = runtime(PermissionConfig::default(), PermissionMode::Default);
        let decision = default_runtime.evaluate("mcp__repo_tools__read_file", &json!({}));
        assert!(matches!(decision, PermissionDecision::Ask { .. }));

        let allow_runtime = runtime(
            PermissionConfig {
                allow: vec!["Mcp(repo_tools/read_file)".to_string()],
                ..Default::default()
            },
            PermissionMode::Default,
        );
        let decision = allow_runtime.evaluate("mcp__repo_tools__read_file", &json!({}));
        assert_eq!(decision, PermissionDecision::Allow);

        let deny_runtime = runtime(
            PermissionConfig {
                deny: vec!["Mcp(repo_tools/*)".to_string()],
                ..Default::default()
            },
            PermissionMode::Default,
        );
        let decision = deny_runtime.evaluate("mcp__repo_tools__read_file", &json!({}));
        assert!(matches!(decision, PermissionDecision::Deny { .. }));
    }

    #[test]
    fn mcp_startup_defaults_to_ask_and_matches_rules() {
        let args = json!({"server": "repo_tools", "transport": "stdio"});
        let default_runtime = runtime(PermissionConfig::default(), PermissionMode::Default);
        let decision = default_runtime.evaluate("mcp_startup", &args);
        assert!(matches!(decision, PermissionDecision::Ask { .. }));

        let allow_runtime = runtime(
            PermissionConfig {
                allow: vec!["McpStartup(repo_tools)".to_string()],
                ..Default::default()
            },
            PermissionMode::Default,
        );
        let decision = allow_runtime.evaluate("mcp_startup", &args);
        assert_eq!(decision, PermissionDecision::Allow);

        let deny_runtime = runtime(
            PermissionConfig {
                deny: vec!["McpStartup(repo_*)".to_string()],
                ..Default::default()
            },
            PermissionMode::Default,
        );
        let decision = deny_runtime.evaluate("mcp_startup", &args);
        assert!(matches!(decision, PermissionDecision::Deny { .. }));
    }

    #[tokio::test]
    async fn dont_ask_denies_actions_that_would_prompt() {
        let runtime = runtime(
            PermissionConfig {
                ask: vec!["ExecCommand(npm publish *)".to_string()],
                ..Default::default()
            },
            PermissionMode::DontAsk,
        );
        let output = runtime
            .authorize(
                "call-1",
                "exec_command",
                &json!({"cmd": "npm publish --dry-run"}),
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
}
