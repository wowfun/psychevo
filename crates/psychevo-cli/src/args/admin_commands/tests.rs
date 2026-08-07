#[cfg(all(feature = "gateway", feature = "desktop"))]
use std::path::Path;

use clap::{CommandFactory, Parser};

use crate::args::{
    Cli, Commands, ConfigArgs, ConfigCommand, ConfigExtensionArgs, ExtensionInstallArgs,
    ExtensionListArgs, ExtensionUpdateArgs, HookKeyArgs, HooksArgs, HooksCommand, HooksListArgs,
    PluginArgs, PluginCommand, PluginListArgs, SkillsArgs, SkillsCommand, SkillsInstallArgs,
    SkillsListArgs,
};
#[cfg(all(feature = "gateway", feature = "desktop"))]
use crate::args::{DesktopArgs, GatewayOpenArgs, TuiArgs, WebArgs};

#[test]
fn exposes_compiled_version_without_runtime_initialization() {
    for flag in ["--version", "-V"] {
        let error = Cli::try_parse_from(["pevo", flag]).expect_err("display version");
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayVersion);
        assert!(
            error.to_string().contains(env!("CARGO_PKG_VERSION")),
            "{error}"
        );
    }
}

#[test]
fn parses_singular_skill_and_defers_unknown_words_to_extension_dispatch() {
    let cli = Cli::try_parse_from(["pevo", "skill", "list", "--json"]).expect("skill");
    assert!(matches!(
        cli.command,
        Some(Commands::Skill(SkillsArgs {
            command: Some(SkillsCommand::List(SkillsListArgs { json: true, .. }))
        }))
    ));
    assert!(matches!(
        Cli::try_parse_from(["pevo", "skills", "list"])
            .expect("unknown top-level words are Extension candidates")
            .command,
        Some(Commands::External(arguments)) if arguments == ["skills", "list"]
    ));
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
    let add = Cli::try_parse_from(["pevo", "plugin", "add", "review@local"])
        .expect("marketplace-qualified plugin add");
    assert!(matches!(
        add.command,
        Some(Commands::Plugin(PluginArgs {
            command: Some(PluginCommand::Add(_))
        }))
    ));
    assert!(Cli::try_parse_from(["pevo", "plugin", "install", "/tmp/plugin"]).is_err());
    assert!(Cli::try_parse_from(["pevo", "plugin", "uninstall", "review"]).is_err());
    assert!(
        Cli::try_parse_from([
            "pevo",
            "plugin",
            "marketplace",
            "add",
            "/tmp/plugins",
            "--name",
            "local",
            "--kind",
            "local",
            "--json"
        ])
        .is_ok()
    );
    assert!(
        Cli::try_parse_from([
            "pevo",
            "plugin",
            "marketplace",
            "upgrade",
            "local",
            "--json"
        ])
        .is_ok()
    );
}

#[test]
fn parses_pi_style_extension_lifecycle_and_external_commands() {
    let install = Cli::try_parse_from(["pevo", "install", "./echo", "-l", "--json"])
        .expect("extension install");
    assert!(matches!(
        install.command,
        Some(Commands::Install(ExtensionInstallArgs {
            local: true,
            json: true,
            ..
        }))
    ));

    let list = Cli::try_parse_from(["pevo", "list", "--local", "--json"]).expect("extension list");
    assert!(matches!(
        list.command,
        Some(Commands::List(ExtensionListArgs {
            local: true,
            json: true
        }))
    ));

    let update = Cli::try_parse_from(["pevo", "update", "--extensions", "--json"])
        .expect("extension update");
    assert!(matches!(
        update.command,
        Some(Commands::Update(ExtensionUpdateArgs {
            extensions: true,
            json: true,
            ..
        }))
    ));
    assert!(Cli::try_parse_from(["pevo", "update", "review", "--all"]).is_err());

    let external = Cli::try_parse_from(["pevo", "-e", "./echo", "echo", "a b", "literal"])
        .expect("temporary direct command");
    assert_eq!(
        external.extension_paths,
        vec![std::path::PathBuf::from("./echo")]
    );
    assert!(matches!(
        external.command,
        Some(Commands::External(arguments))
            if arguments == ["echo", "a b", "literal"]
    ));

    let config = Cli::try_parse_from([
        "pevo",
        "config",
        "extension",
        "example.echo",
        "--local",
        "--disable",
        "--json",
    ])
    .expect("Extension config");
    assert!(matches!(
        config.command,
        Some(Commands::Config(ConfigArgs {
            command: ConfigCommand::Extension(ConfigExtensionArgs {
                local: true,
                disable: true,
                json: true,
                ..
            })
        }))
    ));
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
    let cli =
        Cli::try_parse_from(["pevo", "hooks", "trust", "hk_abc", "--json"]).expect("hooks trust");
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
        Cli::try_parse_from(["pevo", "skill", "install", "/tmp/reviewer", "--project"]).is_err()
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
fn compiled_feature_commands_match_selected_surface() {
    let command = Cli::command();
    let names = command
        .get_subcommands()
        .map(|command| command.get_name())
        .collect::<Vec<_>>();

    assert!(names.contains(&"install"));
    assert_eq!(names.contains(&"acp"), cfg!(feature = "acp"));
    for name in ["run", "tui", "web", "serve", "gateway"] {
        assert_eq!(names.contains(&name), cfg!(feature = "gateway"), "{name}");
    }
    assert_eq!(names.contains(&"desktop"), cfg!(feature = "desktop"));
}

#[test]
#[cfg(all(feature = "gateway", feature = "desktop"))]
fn parses_new_cli_command_families() {
    let cli = Cli::try_parse_from(["pevo", "-C", "work"]).expect("default");
    assert_eq!(cli.cd.as_deref(), Some(Path::new("work")));
    assert!(cli.command.is_none());
    let cli = Cli::try_parse_from(["pevo", "tui", "--cd", "work"]).expect("tui");
    assert!(matches!(
        cli.command,
        Some(Commands::Tui(TuiArgs { cd: Some(_), .. }))
    ));
    let cli =
        Cli::try_parse_from(["pevo", "web", "--no-browser", "--print-url"]).expect("web open");
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
    assert!(Cli::try_parse_from(["pevo", "web", "restart", "--bind", "127.0.0.1:58081"]).is_ok());
    let cli = Cli::try_parse_from(["pevo", "web", "-C", "work"]).expect("web cwd");
    assert!(matches!(
        cli.command,
        Some(Commands::Web(WebArgs {
            command: None,
            open: GatewayOpenArgs { cd: Some(_), .. }
        }))
    ));
    let cli = Cli::try_parse_from(["pevo", "desktop", "--cd", "work"]).expect("desktop");
    assert!(matches!(
        cli.command,
        Some(Commands::Desktop(DesktopArgs { cd: Some(_) }))
    ));
    assert!(Cli::try_parse_from(["pevo", "gateway", "open", "-C", "work"]).is_ok());
    for args in [
        ["pevo", "tui", "--dir", "work"],
        ["pevo", "web", "--dir", "work"],
        ["pevo", "desktop", "--dir", "work"],
    ] {
        assert!(Cli::try_parse_from(args).is_err(), "{args:?}");
    }
    assert!(Cli::try_parse_from(["pevo", "gateway", "open", "--dir", "work"]).is_err());
    assert!(Cli::try_parse_from(["pevo", "run", "--dir", "work", "hello"]).is_ok());
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
    assert!(Cli::try_parse_from(["pevo", "run", "--permission-mode", "dontAsk", "hello"]).is_ok());
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
        Cli::try_parse_from(["pevo", "agent", "run", "reviewer", "-f", "json", "hello"]).is_ok()
    );
    assert!(Cli::try_parse_from(["pevo", "session", "export", "latest", "-f", "json"]).is_ok());
    assert!(
        Cli::try_parse_from(["pevo", "session", "export", "latest", "--with-reasoning"]).is_err()
    );
    assert!(Cli::try_parse_from(["pevo", "session", "export", "latest", "--full-inputs"]).is_err());
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
    assert!(Cli::try_parse_from(["pevo", "session", "share", "latest", "--last-request"]).is_err());
    assert!(
        Cli::try_parse_from(["pevo", "session", "share", "latest", "--with-reasoning"]).is_err()
    );
    assert!(Cli::try_parse_from(["pevo", "session", "share", "latest", "--full-inputs"]).is_err());
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
        Cli::try_parse_from(["pevo", "auth", "set", "mock", "--api-key-stdin", "--local"]).is_ok()
    );
}
