use std::path::PathBuf;

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
    match xtask.command {
        Command::GatewayProtocol(GatewayProtocolCommand::Generate { check }) => {
            let root = repo_root()?;
            psychevo_gateway_protocol::generate_typescript_and_schema(&root, check)
        }
    }
}

fn repo_root() -> Result<PathBuf> {
    let mut dir = std::env::current_dir().context("read current directory")?;
    loop {
        if dir.join("Cargo.toml").is_file() && dir.join("packages").is_dir() {
            return Ok(dir);
        }
        if !dir.pop() {
            bail!("could not find repository root");
        }
    }
}
