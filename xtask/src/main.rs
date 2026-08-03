mod ci;
mod doctor;
mod host_command;
mod init;
mod live;
mod paths;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "psychevo-xtask")]
struct Xtask {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(subcommand)]
    GatewayProtocol(GatewayProtocolCommand),
    #[command(subcommand)]
    Ci(ci::CiCommand),
    #[command(subcommand)]
    Doctor(doctor::DoctorCommand),
    #[command(subcommand)]
    Init(init::InitCommand),
    #[command(subcommand)]
    Live(live::LiveCommand),
}

#[derive(Debug, Subcommand)]
enum GatewayProtocolCommand {
    Generate {
        #[arg(long)]
        check: bool,
    },
}

fn main() -> Result<()> {
    let xtask = Xtask::parse();
    let root = paths::repo_root()?;
    match xtask.command {
        Command::GatewayProtocol(GatewayProtocolCommand::Generate { check }) => {
            generate_gateway_protocol(&root, check)
        }
        Command::Ci(command) => ci::run(command, &root),
        Command::Doctor(command) => doctor::run(command, &root),
        Command::Init(command) => init::run(command, &root),
        Command::Live(command) => live::run(command, &root),
    }
}

fn generate_gateway_protocol(root: &std::path::Path, check: bool) -> Result<()> {
    psychevo_gateway_protocol::generate_typescript_and_schema(root, check)?;
    let mut args = vec![
        "--dir",
        "packages/protocol",
        "exec",
        "tsx",
        "scripts/generate-validators.ts",
    ];
    if check {
        args.push("--check");
    }
    let mut command = host_command::pnpm(args)?;
    command.current_dir(root);
    let status = command
        .status()
        .context("run Gateway standalone validator generation")?;
    if !status.success() {
        bail!("Gateway standalone validator generation failed with {status}");
    }
    Ok(())
}
