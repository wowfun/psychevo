mod ci;
mod doctor;
mod init;
mod live;
mod paths;

use anyhow::Result;
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
            psychevo_gateway_protocol::generate_typescript_and_schema(&root, check)
        }
        Command::Ci(command) => ci::run(command, &root),
        Command::Doctor(command) => doctor::run(command, &root),
        Command::Init(command) => init::run(command, &root),
        Command::Live(command) => live::run(command, &root),
    }
}
