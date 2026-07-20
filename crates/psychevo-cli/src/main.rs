#![allow(clippy::module_inception)]

use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::Parser;

pub(crate) mod args;
pub(crate) mod command_registry;
pub(crate) mod commands;
pub(crate) mod env;
pub(crate) mod profiles;
pub(crate) mod provider_setup;
pub(crate) mod tui;

use args::{Cli, Commands};
use commands::agent::run_agent_command;
use commands::auth::run_auth_command;
use commands::config::run_config_command;
use commands::context::run_context_command;
use commands::desktop::run_desktop_command;
use commands::doctor::run_doctor_command;
use commands::gateway::run_gateway_command;
use commands::hooks::run_hooks_command;
use commands::init::run_init_command;
use commands::mcp::run_mcp_command;
use commands::model::run_model_command;
use commands::plugin::run_plugin_command;
use commands::profile::run_profile_command;
use commands::run::run_run_command;
use commands::serve::run_serve_command;
use commands::session::run_session_command;
use commands::setup::run_setup_command;
use commands::skills::run_skills_command;
use commands::stats::run_stats_command;
use commands::tool::run_tool_command;

#[tokio::main]
pub(crate) async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(1)
        }
    }
}

pub(crate) async fn run() -> Result<ExitCode> {
    debug_assert!(
        command_registry::CLI_COMMANDS
            .iter()
            .all(|spec| spec.surface == command_registry::CommandSurface::PevoCli)
    );
    let mut cli = Cli::parse();
    let default_tui_cd = apply_root_cd(cli.cd.take(), &mut cli.command)?;
    profiles::set_cli_profile_override(cli.profile.clone())?;
    match cli.command {
        None => run_default_command(default_tui_cd).await,
        Some(Commands::Init(args)) => run_init_command(args).await,
        Some(Commands::Profile(args)) => run_profile_command(args),
        Some(Commands::Agent(args)) => run_agent_command(args).await,
        Some(Commands::Skill(args)) => run_skills_command(args),
        Some(Commands::Plugin(args)) => run_plugin_command(args),
        Some(Commands::Hooks(args)) => run_hooks_command(args),
        Some(Commands::Tool(args)) => run_tool_command(args),
        Some(Commands::Run(args)) => run_run_command(args).await,
        Some(Commands::Stats(args)) => run_stats_command(args),
        Some(Commands::Context(args)) => run_context_command(args),
        Some(Commands::Session(args)) => run_session_command(args),
        Some(Commands::Model(args)) => run_model_command(args).await,
        Some(Commands::Config(args)) => run_config_command(args),
        Some(Commands::Auth(args)) => run_auth_command(args),
        Some(Commands::Acp(args)) => {
            if args.setup {
                println!(
                    "Run `pevo auth setup --provider <id> --model <model> --base-url <url> --api-key-stdin` or add `--no-auth` for explicit no-auth providers."
                );
                return Ok(ExitCode::SUCCESS);
            }
            let env_map = env::inherited_env();
            let cwd = std::env::current_dir()?;
            psychevo_acp::run_stdio(psychevo_acp::AcpOptions::from_env_map(env_map, cwd)).await?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Commands::Mcp(args)) => run_mcp_command(args).await,
        Some(Commands::Tui(args)) => tui::run_tui_command(&args).await,
        Some(Commands::Web(args)) => commands::gateway::run_web_command(args).await,
        Some(Commands::Desktop(args)) => run_desktop_command(args).await,
        Some(Commands::Serve(args)) => run_serve_command(args).await,
        Some(Commands::Gateway(args)) => run_gateway_command(args).await,
        Some(Commands::Doctor(args)) => run_doctor_command(args).await,
        Some(Commands::Setup(args)) => run_setup_command(args).await,
    }
}

fn apply_root_cd(
    root_cd: Option<PathBuf>,
    command: &mut Option<Commands>,
) -> Result<Option<PathBuf>> {
    let Some(root_cd) = root_cd else {
        return Ok(None);
    };
    match command {
        None => Ok(Some(root_cd)),
        Some(Commands::Tui(args)) => {
            args.cd.get_or_insert(root_cd);
            Ok(None)
        }
        Some(Commands::Web(args)) if args.command.is_none() => {
            merge_open_cd(&mut args.open, root_cd)?;
            Ok(None)
        }
        Some(Commands::Desktop(args)) => {
            args.cd.get_or_insert(root_cd);
            Ok(None)
        }
        Some(Commands::Gateway(args)) => {
            match &mut args.command {
                Some(crate::args::GatewayCommand::Open(args)) => {
                    merge_open_cd(args, root_cd)?;
                }
                None => {
                    args.command = Some(crate::args::GatewayCommand::Open(
                        crate::args::GatewayOpenArgs {
                            cd: Some(root_cd),
                            default_workspace: false,
                            bind: None,
                            no_browser: false,
                            print_url: false,
                        },
                    ));
                }
                Some(_) => bail!("`-C/--cd` can only be used with `gateway open`"),
            }
            Ok(None)
        }
        Some(Commands::Web(_)) => {
            bail!("`-C/--cd` opens a workspace and cannot be used with Web lifecycle commands")
        }
        Some(_) => bail!("`-C/--cd` is only supported when opening the TUI or GUI"),
    }
}

fn merge_open_cd(args: &mut crate::args::GatewayOpenArgs, root_cd: PathBuf) -> Result<()> {
    if args.cd.is_none() {
        if args.default_workspace {
            bail!("`-C/--cd` cannot be used with `--default-workspace`");
        }
        args.cd = Some(root_cd);
    }
    Ok(())
}

async fn run_default_command(cd: Option<PathBuf>) -> Result<ExitCode> {
    if io::stdin().is_terminal() && io::stdout().is_terminal() {
        let args = crate::args::TuiArgs {
            cd,
            ..Default::default()
        };
        return tui::run_tui_command(&args).await;
    }
    eprintln!("pevo with no command requires an interactive terminal.");
    eprintln!("Use an explicit command instead:");
    eprintln!("  pevo tui");
    eprintln!("  pevo run <prompt>");
    eprintln!("  pevo web");
    eprintln!("  pevo --help");
    Ok(ExitCode::from(2))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn root_cd_routes_to_default_tui() {
        let mut command = None;

        let default_cd =
            apply_root_cd(Some(PathBuf::from("workspace")), &mut command).expect("root cwd");

        assert_eq!(default_cd.as_deref(), Some(Path::new("workspace")));
    }

    #[test]
    fn command_local_cd_wins_over_root_cd() {
        let mut command = Some(Commands::Tui(crate::args::TuiArgs {
            cd: Some(PathBuf::from("local")),
            ..Default::default()
        }));

        apply_root_cd(Some(PathBuf::from("root")), &mut command).expect("root cwd");

        assert!(matches!(
            command,
            Some(Commands::Tui(crate::args::TuiArgs { cd: Some(path), .. }))
                if path == Path::new("local")
        ));
    }

    #[test]
    fn root_cd_routes_to_default_gateway_open() {
        let mut command = Some(Commands::Gateway(crate::args::GatewayArgs {
            command: None,
        }));

        apply_root_cd(Some(PathBuf::from("workspace")), &mut command).expect("root cwd");

        assert!(matches!(
            command,
            Some(Commands::Gateway(crate::args::GatewayArgs {
                command: Some(crate::args::GatewayCommand::Open(
                    crate::args::GatewayOpenArgs { cd: Some(path), .. }
                )),
            })) if path == Path::new("workspace")
        ));
    }

    #[test]
    fn root_cd_routes_to_native_desktop() {
        let mut command = Some(Commands::Desktop(crate::args::DesktopArgs { cd: None }));

        apply_root_cd(Some(PathBuf::from("workspace")), &mut command).expect("root cwd");

        assert!(matches!(
            command,
            Some(Commands::Desktop(crate::args::DesktopArgs { cd: Some(path) }))
                if path == Path::new("workspace")
        ));
    }

    #[test]
    fn root_cd_rejects_web_lifecycle_commands() {
        let mut command = Some(Commands::Web(crate::args::WebArgs {
            open: crate::args::GatewayOpenArgs {
                cd: None,
                default_workspace: false,
                bind: None,
                no_browser: false,
                print_url: false,
            },
            command: Some(crate::args::WebCommand::Start(
                crate::args::GatewayStartArgs { bind: None },
            )),
        }));

        let error = apply_root_cd(Some(PathBuf::from("workspace")), &mut command)
            .expect_err("Web lifecycle command must reject cwd");

        assert_eq!(
            error.to_string(),
            "`-C/--cd` opens a workspace and cannot be used with Web lifecycle commands"
        );
    }

    #[test]
    fn root_cd_rejects_non_ui_commands() {
        let mut command = Some(Commands::Doctor(crate::args::DoctorArgs {
            json: false,
            live: false,
        }));

        let error = apply_root_cd(Some(PathBuf::from("workspace")), &mut command)
            .expect_err("non-UI command must reject cwd");

        assert_eq!(
            error.to_string(),
            "`-C/--cd` is only supported when opening the TUI or GUI"
        );
    }
}
