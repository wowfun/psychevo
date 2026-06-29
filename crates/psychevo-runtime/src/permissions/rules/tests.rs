
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
    fn system_shutdown_hardline_matches_command_positions_only() {
        let runtime = runtime(
            PermissionConfig::default(),
            PermissionMode::BypassPermissions,
        );
        for command in [
            "shutdown -h now",
            "sudo reboot",
            "env X=1 poweroff",
            "exec halt",
            "nohup shutdown now",
            "setsid reboot",
            "systemctl reboot",
        ] {
            let decision = runtime.evaluate("exec_command", &json!({"cmd": command}));
            assert!(
                matches!(decision, PermissionDecision::Deny { .. }),
                "{command}"
            );
        }
    }

    #[test]
    fn system_shutdown_words_in_arguments_do_not_hardline_deny() {
        let runtime = runtime(
            PermissionConfig::default(),
            PermissionMode::BypassPermissions,
        );
        for command in [
            r#"sqlite3 /repo/feeds/.cache/hn.db "UPDATE stories SET content = 'system halted after a kernel panic' WHERE id = 1;""#,
            "echo reboot",
            "grep shutdown log.txt",
            "printf 'poweroff is a command name, not this command'",
        ] {
            let decision = runtime.evaluate("exec_command", &json!({"cmd": command}));
            assert_eq!(decision, PermissionDecision::Allow, "{command}");
        }
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
    fn heredoc_content_ampersands_are_not_background_wrappers() {
        let runtime = runtime(
            PermissionConfig::default(),
            PermissionMode::BypassPermissions,
        );
        let command = "cat > /tmp/fixnull.c <<'EOF'\nint flags = value & mask;\nEOF";
        let decision = runtime.evaluate("exec_command", &json!({"cmd": command}));
        assert_eq!(decision, PermissionDecision::Allow);
    }

    #[test]
    fn outside_cwd_exec_cwd_defaults_to_ask() {
        let runtime = runtime(PermissionConfig::default(), PermissionMode::Default);
        let decision = runtime.evaluate("exec_command", &json!({"cmd": "pwd", "cwd": "/tmp"}));
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
                &json!({"path": "/tmp/outside-cwd.txt"}),
            )
            .await
            .expect_err("outside cwd read should need a handler");
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
