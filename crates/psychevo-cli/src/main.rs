use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;

mod args;
mod commands;
mod env;
mod tui;

use args::{Cli, Commands};
use commands::init::run_init_command;
use commands::run::run_run_command;
use commands::smoke::run_smoke_command;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(1)
        }
    }
}

async fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init(args) => run_init_command(args),
        Commands::Smoke(args) => run_smoke_command(args).await,
        Commands::Run(args) => run_run_command(args).await,
        Commands::Tui(args) => tui::run_tui_command(&args).await,
    }
}
