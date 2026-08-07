#[cfg(feature = "gateway")]
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::{CommandFactory, Parser};
use psychevo::command_registry::{CLI_COMMANDS, CommandSurface};
use psychevo::extensions::{ExtensionCommandCatalog, ExtensionStore, load_extension_manifest};

pub(crate) mod args;
pub(crate) mod commands;
pub(crate) mod env;
pub(crate) mod profiles;
pub(crate) mod provider_setup;
#[cfg(feature = "gateway")]
pub(crate) mod tui;

use args::{Cli, Commands};
use commands::agent::run_agent_command;
use commands::auth::run_auth_command;
use commands::config::run_config_command;
use commands::context::run_context_command;
#[cfg(feature = "desktop")]
use commands::desktop::run_desktop_command;
use commands::doctor::run_doctor_command;
use commands::extension::{
    run_external_command, run_install_command, run_list_command, run_remove_command,
    run_update_command,
};
#[cfg(feature = "gateway")]
use commands::gateway::run_gateway_command;
use commands::hooks::run_hooks_command;
use commands::init::run_init_command;
use commands::mcp::run_mcp_command;
use commands::model::run_model_command;
use commands::plugin::run_plugin_command;
use commands::profile::run_profile_command;
#[cfg(feature = "gateway")]
use commands::run::run_run_command;
#[cfg(feature = "gateway")]
use commands::serve::run_serve_command;
use commands::session::run_session_command;
use commands::setup::run_setup_command;
use commands::skills::run_skills_command;
use commands::stats::run_stats_command;
use commands::tool::run_tool_command;

#[cfg(windows)]
pub(crate) fn main() -> ExitCode {
    match std::thread::Builder::new()
        .name("pevo-main".to_string())
        .stack_size(8 * 1024 * 1024)
        .spawn(runtime_main)
    {
        Ok(thread) => match thread.join() {
            Ok(code) => code,
            Err(panic) => std::panic::resume_unwind(panic),
        },
        Err(err) => {
            eprintln!("error: failed to start pevo runtime: {err}");
            ExitCode::from(1)
        }
    }
}

#[cfg(not(windows))]
pub(crate) fn main() -> ExitCode {
    runtime_main()
}

#[tokio::main]
async fn runtime_main() -> ExitCode {
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
        CLI_COMMANDS
            .iter()
            .all(|spec| spec.surface == CommandSurface::PevoCli)
    );
    if render_root_help_if_requested()? {
        return Ok(ExitCode::SUCCESS);
    }
    let mut cli = Cli::parse();
    let default_tui_cd = apply_root_cd(cli.cd.take(), &mut cli.command)?;
    let extension_paths = std::mem::take(&mut cli.extension_paths);
    let extension_paths_supported = match &cli.command {
        None | Some(Commands::External(_)) => true,
        #[cfg(feature = "gateway")]
        Some(Commands::Tui(_)) => true,
        _ => false,
    };
    if !extension_paths.is_empty() && !extension_paths_supported {
        bail!("`-e` is supported only with a direct Extension command or the TUI");
    }
    profiles::set_cli_profile_override(cli.profile.clone())?;
    match cli.command {
        None => run_default_command(default_tui_cd, extension_paths).await,
        Some(Commands::Init(args)) => run_init_command(args).await,
        Some(Commands::Profile(args)) => run_profile_command(args).await,
        Some(Commands::Agent(args)) => run_agent_command(args).await,
        Some(Commands::Skill(args)) => run_skills_command(args),
        Some(Commands::Plugin(args)) => run_plugin_command(args).await,
        Some(Commands::Install(args)) if extension_paths.is_empty() => {
            run_install_command(args).await
        }
        Some(Commands::Remove(args)) if extension_paths.is_empty() => run_remove_command(args),
        Some(Commands::List(args)) if extension_paths.is_empty() => run_list_command(args),
        Some(Commands::Update(args)) if extension_paths.is_empty() => {
            run_update_command(args).await
        }
        Some(Commands::Hooks(args)) => run_hooks_command(args).await,
        Some(Commands::Tool(args)) => run_tool_command(args).await,
        #[cfg(feature = "gateway")]
        Some(Commands::Run(args)) => run_run_command(args).await,
        Some(Commands::Stats(args)) => run_stats_command(args).await,
        Some(Commands::Context(args)) => run_context_command(args).await,
        Some(Commands::Session(args)) => run_session_command(args).await,
        Some(Commands::Model(args)) => run_model_command(args).await,
        Some(Commands::Config(args)) => run_config_command(args).await,
        Some(Commands::Auth(args)) => run_auth_command(args).await,
        #[cfg(feature = "acp")]
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
        #[cfg(feature = "gateway")]
        Some(Commands::Tui(args)) => tui::run_tui_command(&args, &extension_paths).await,
        #[cfg(feature = "gateway")]
        Some(Commands::Web(args)) => commands::gateway::run_web_command(args).await,
        #[cfg(feature = "desktop")]
        Some(Commands::Desktop(args)) => run_desktop_command(args).await,
        #[cfg(feature = "gateway")]
        Some(Commands::Serve(args)) => run_serve_command(args).await,
        #[cfg(feature = "gateway")]
        Some(Commands::Gateway(args)) => run_gateway_command(args).await,
        Some(Commands::Doctor(args)) => run_doctor_command(args).await,
        Some(Commands::Setup(args)) => run_setup_command(args).await,
        Some(Commands::External(arguments)) => {
            run_external_command(arguments, extension_paths).await
        }
        Some(_) => bail!("this command is unavailable in the selected pevo build"),
    }
}

fn render_root_help_if_requested() -> Result<bool> {
    let args = std::env::args_os().collect::<Vec<_>>();
    if args.len() != 2 || !matches!(args[1].to_str(), Some("-h" | "--help")) {
        return Ok(false);
    }
    let env = env::inherited_env();
    let cwd = std::env::current_dir()?;
    let home = env::resolve_psychevo_home(&env, &cwd)?;
    let mut command = Cli::command();
    if home.join("config.toml").is_file() {
        let store = ExtensionStore::new(home, cwd);
        let mut manifests = Vec::new();
        for record in store
            .effective_records()?
            .into_iter()
            .filter(|record| record.enabled)
        {
            let fingerprint = psychevo::plugins::external_plugin_fingerprint(
                Some(&record.package_root),
                &record.id,
                Some(&record.version),
            )?;
            if fingerprint == record.fingerprint && fingerprint == record.trusted_fingerprint {
                manifests.push(load_extension_manifest(&record.package_root)?);
            }
        }
        let builtins = command
            .get_subcommands()
            .map(|command| command.get_name())
            .collect::<Vec<_>>();
        ExtensionCommandCatalog::build(&manifests, &builtins)?;
        let mut contributions = manifests
            .iter()
            .flat_map(|manifest| {
                manifest
                    .contributions
                    .commands
                    .iter()
                    .map(move |contribution| {
                        format!(
                            "  {:<18} {} ({})",
                            contribution.name, contribution.summary, manifest.id
                        )
                    })
            })
            .collect::<Vec<_>>();
        contributions.sort();
        if !contributions.is_empty() {
            command = command.after_help(format!("Extensions:\n{}", contributions.join("\n")));
        }
    }
    command.print_help()?;
    println!();
    Ok(true)
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
        #[cfg(feature = "gateway")]
        Some(Commands::Tui(args)) => {
            args.cd.get_or_insert(root_cd);
            Ok(None)
        }
        #[cfg(feature = "gateway")]
        Some(Commands::Web(args)) if args.command.is_none() => {
            merge_open_cd(&mut args.open, root_cd)?;
            Ok(None)
        }
        #[cfg(feature = "desktop")]
        Some(Commands::Desktop(args)) => {
            args.cd.get_or_insert(root_cd);
            Ok(None)
        }
        #[cfg(feature = "gateway")]
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
        #[cfg(feature = "gateway")]
        Some(Commands::Web(_)) => {
            bail!("`-C/--cd` opens a workspace and cannot be used with Web lifecycle commands")
        }
        Some(_) => bail!("`-C/--cd` is only supported when opening the TUI or GUI"),
    }
}

#[cfg(feature = "gateway")]
fn merge_open_cd(args: &mut crate::args::GatewayOpenArgs, root_cd: PathBuf) -> Result<()> {
    if args.cd.is_none() {
        if args.default_workspace {
            bail!("`-C/--cd` cannot be used with `--default-workspace`");
        }
        args.cd = Some(root_cd);
    }
    Ok(())
}

#[cfg(feature = "gateway")]
async fn run_default_command(
    cd: Option<PathBuf>,
    temporary_extension_paths: Vec<PathBuf>,
) -> Result<ExitCode> {
    if !temporary_extension_paths.is_empty()
        || (io::stdin().is_terminal() && io::stdout().is_terminal())
    {
        let args = crate::args::TuiArgs {
            cd,
            ..Default::default()
        };
        return tui::run_tui_command(&args, &temporary_extension_paths).await;
    }
    eprintln!("pevo with no command requires an interactive terminal.");
    eprintln!("Use an explicit command instead:");
    eprintln!("  pevo tui");
    eprintln!("  pevo run <prompt>");
    eprintln!("  pevo web");
    eprintln!("  pevo --help");
    Ok(ExitCode::from(2))
}

#[cfg(not(feature = "gateway"))]
async fn run_default_command(
    _cd: Option<PathBuf>,
    _temporary_extension_paths: Vec<PathBuf>,
) -> Result<ExitCode> {
    eprintln!("This pevo build omits the TUI and Gateway surfaces.");
    eprintln!("Use an explicit installed Extension command or `pevo --help`.");
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
    #[cfg(feature = "gateway")]
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
    #[cfg(feature = "gateway")]
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
    #[cfg(feature = "desktop")]
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
    #[cfg(feature = "gateway")]
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
