#![allow(clippy::module_inception)]

use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;

pub(crate) mod args;
pub(crate) mod command_registry;
pub(crate) mod commands;
pub(crate) mod env;
pub(crate) mod tui;

use args::{Cli, Commands};
use commands::agent::run_agent_command;
use commands::auth::run_auth_command;
use commands::config::run_config_command;
use commands::context::run_context_command;
use commands::init::run_init_command;
use commands::model::run_model_command;
use commands::run::run_run_command;
use commands::session::run_session_command;
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
    let cli = Cli::parse();
    match cli.command {
        Commands::Init(args) => run_init_command(args),
        Commands::Agent(args) => run_agent_command(args).await,
        Commands::Skill(args) => run_skills_command(args),
        Commands::Tool(args) => run_tool_command(args),
        Commands::Run(args) => run_run_command(args).await,
        Commands::Stats(args) => run_stats_command(args),
        Commands::Context(args) => run_context_command(args),
        Commands::Session(args) => run_session_command(args),
        Commands::Model(args) => run_model_command(args).await,
        Commands::Config(args) => run_config_command(args),
        Commands::Auth(args) => run_auth_command(args),
        Commands::Acp(args) => {
            if args.setup {
                println!(
                    "Run `pevo auth setup --provider <id> --model <model> --base-url <url> --api-key-stdin` or add `--no-auth` for explicit no-auth providers."
                );
                return Ok(ExitCode::SUCCESS);
            }
            psychevo_acp::run_stdio(psychevo_acp::AcpOptions::from_env()).await?;
            Ok(ExitCode::SUCCESS)
        }
        Commands::Tui(args) => tui::run_tui_command(&args).await,
    }
}
