#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::*;

    #[test]
    fn parses_singular_skill_and_rejects_plural_skills() {
        let cli = Cli::try_parse_from(["pevo", "skill", "list", "--json"]).expect("skill");
        assert!(matches!(
            cli.command,
            Some(Commands::Skill(SkillsArgs {
                command: Some(SkillsCommand::List(SkillsListArgs { json: true, .. }))
            }))
        ));
        assert!(Cli::try_parse_from(["pevo", "skills", "list"]).is_err());
    }

    #[test]
    fn parses_singular_plugin_and_rejects_plural_plugins() {
        let cli = Cli::try_parse_from(["pevo", "plugin", "list", "--json"]).expect("plugin");
        assert!(matches!(
            cli.command,
            Some(Commands::Plugin(PluginArgs {
                command: Some(PluginCommand::List(PluginListArgs { json: true }))
            }))
        ));
        assert!(Cli::try_parse_from(["pevo", "plugins", "list"]).is_err());
        assert!(
            Cli::try_parse_from([
                "pevo",
                "plugin",
                "install",
                "/tmp/plugin",
                "--local",
                "--global"
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "pevo",
                "plugin",
                "marketplace",
                "add",
                "local",
                "/tmp/plugins",
                "--kind",
                "local",
                "--json"
            ])
            .is_ok()
        );
    }

    #[test]
    fn parses_hooks_commands() {
        let cli = Cli::try_parse_from(["pevo", "hooks", "list", "--json"]).expect("hooks");
        assert!(matches!(
            cli.command,
            Some(Commands::Hooks(HooksArgs {
                command: Some(HooksCommand::List(HooksListArgs { json: true }))
            }))
        ));
        let cli = Cli::try_parse_from(["pevo", "hooks", "trust", "hk_abc", "--json"])
            .expect("hooks trust");
        assert!(matches!(
            cli.command,
            Some(Commands::Hooks(HooksArgs {
                command: Some(HooksCommand::Trust(HookKeyArgs { key, json: true }))
            })) if key == "hk_abc"
        ));
    }

    #[test]
    fn parses_local_scope_and_rejects_project_alias() {
        let cli = Cli::try_parse_from([
            "pevo",
            "skill",
            "install",
            "/tmp/reviewer",
            "--local",
            "--force",
        ])
        .expect("local skill install");
        assert!(matches!(
            cli.command,
            Some(Commands::Skill(SkillsArgs {
                command: Some(SkillsCommand::Install(SkillsInstallArgs {
                    local: true,
                    force: true,
                    ..
                }))
            }))
        ));
        assert!(
            Cli::try_parse_from(["pevo", "skill", "install", "/tmp/reviewer", "--project"])
                .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "pevo",
                "config",
                "provider",
                "add",
                "--id",
                "mock",
                "--label",
                "Mock",
                "--base-url",
                "http://127.0.0.1/v1",
                "--project",
            ])
            .is_err()
        );
    }

    #[test]
    fn parses_new_cli_command_families() {
        assert!(
            Cli::try_parse_from(["pevo"])
                .expect("default")
                .command
                .is_none()
        );
        let cli = Cli::try_parse_from(["pevo", "web", "--no-browser", "--print-url"])
            .expect("web open");
        assert!(matches!(
            cli.command,
            Some(Commands::Web(WebArgs {
                command: None,
                open: GatewayOpenArgs {
                    no_browser: true,
                    print_url: true,
                    ..
                }
            }))
        ));
        assert!(Cli::try_parse_from(["pevo", "web", "start"]).is_ok());
        assert!(Cli::try_parse_from(["pevo", "web", "stop"]).is_ok());
        assert!(
            Cli::try_parse_from(["pevo", "web", "restart", "--bind", "127.0.0.1:58081"])
                .is_ok()
        );
        let cli = Cli::try_parse_from(["pevo", "desktop", "--dir", "work"]).expect("desktop");
        assert!(matches!(
            cli.command,
            Some(Commands::Desktop(DesktopArgs {
                dir: Some(_),
            }))
        ));
        assert!(Cli::try_parse_from(["pevo", "doctor", "--json"]).is_ok());
        assert!(Cli::try_parse_from(["pevo", "doctor", "--live"]).is_ok());
        assert!(Cli::try_parse_from(["pevo", "setup", "--dry-run"]).is_ok());
        assert!(Cli::try_parse_from(["pevo", "session", "list", "--archived", "--json"]).is_ok());
        assert!(
            Cli::try_parse_from([
                "pevo",
                "session",
                "export",
                "latest",
                "--format",
                "json",
                "--include",
                "messages,reasoning,provider-input-evidence,last-provider-request,last-provider-response",
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "pevo",
                "session",
                "export",
                "latest",
                "-i",
                "h,m,r,pie,lpr,last-provider-response",
            ])
            .is_ok()
        );
        assert!(Cli::try_parse_from(["pevo", "run", "-f", "json", "hello"]).is_ok());
        assert!(
            Cli::try_parse_from(["pevo", "run", "--permission-mode", "dontAsk", "hello"]).is_ok()
        );
        assert!(
            Cli::try_parse_from(["pevo", "run", "--dangerously-skip-permissions", "hello"]).is_ok()
        );
        assert!(Cli::try_parse_from(["pevo", "run", "--project-context", "cwd", "hello"]).is_ok());
        assert!(Cli::try_parse_from(["pevo", "run", "--isolated", "hello"]).is_ok());
        assert!(
            Cli::try_parse_from([
                "pevo",
                "run",
                "--runtime",
                "opencode",
                "--runtime-option",
                "mode=build",
                "hello"
            ])
            .is_ok()
        );
        assert!(Cli::try_parse_from(["pevo", "run", "--runtime-option", "mode", "hello"]).is_err());
        assert!(
            Cli::try_parse_from([
                "pevo",
                "run",
                "--isolated",
                "--project-context",
                "off",
                "hello"
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from(["pevo", "agent", "run", "reviewer", "-f", "json", "hello"])
                .is_ok()
        );
        assert!(Cli::try_parse_from(["pevo", "session", "export", "latest", "-f", "json"]).is_ok());
        assert!(
            Cli::try_parse_from(["pevo", "session", "export", "latest", "--with-reasoning"])
                .is_err()
        );
        assert!(
            Cli::try_parse_from(["pevo", "session", "export", "latest", "--full-inputs"]).is_err()
        );
        assert!(
            Cli::try_parse_from(["pevo", "session", "export", "latest", "--last-request"]).is_err()
        );
        assert!(
            Cli::try_parse_from(["pevo", "session", "share", "latest", "-i", "h,m,r,pie",]).is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "pevo",
                "session",
                "share",
                "latest",
                "--include",
                "header,messages,reasoning,provider-input-evidence",
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from(["pevo", "session", "export", "latest", "--raw-requests"]).is_err()
        );
        assert!(
            Cli::try_parse_from([
                "pevo",
                "session",
                "export",
                "latest",
                "--include",
                "last-raw-response",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from(["pevo", "session", "share", "latest", "--last-request"]).is_err()
        );
        assert!(
            Cli::try_parse_from(["pevo", "session", "share", "latest", "--with-reasoning"])
                .is_err()
        );
        assert!(
            Cli::try_parse_from(["pevo", "session", "share", "latest", "--full-inputs"]).is_err()
        );
        assert!(
            Cli::try_parse_from([
                "pevo",
                "session",
                "share",
                "latest",
                "--include",
                "last-provider-request",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "pevo",
                "session",
                "share",
                "latest",
                "--include",
                "last-provider-response",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "pevo", "session", "share", "latest", "--output", "share.md", "--json",
            ])
            .is_ok()
        );
        assert!(Cli::try_parse_from(["pevo", "model", "fetch", "mock", "--json"]).is_ok());
        assert!(Cli::try_parse_from(["pevo", "model", "set", "mock/model", "--json"]).is_ok());
        assert!(Cli::try_parse_from(["pevo", "model", "set", "-g", "mock/model"]).is_ok());
        assert!(Cli::try_parse_from(["pevo", "config", "show", "--local", "--json"]).is_ok());
        assert!(Cli::try_parse_from(["pevo", "config", "show", "-g", "--json"]).is_ok());
        assert!(Cli::try_parse_from(["pevo", "config", "permissions", "list", "--json"]).is_ok());
        assert!(
            Cli::try_parse_from([
                "pevo",
                "config",
                "permissions",
                "remove",
                "--kind",
                "allow",
                "--rule",
                "ExecCommand(npm test *)",
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from(["pevo", "auth", "set", "mock", "--api-key-stdin", "--local"])
                .is_ok()
        );
    }
}
