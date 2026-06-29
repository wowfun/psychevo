mod deps;
mod large_files;

use std::path::Path;

use anyhow::Result;
use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub(crate) enum DoctorCommand {
    #[command(subcommand)]
    Deps(deps::DepsCommand),
    LargeFiles(large_files::LargeFilesCommand),
}

pub(crate) fn run(command: DoctorCommand, root: &Path) -> Result<()> {
    match command {
        DoctorCommand::Deps(command) => deps::run(command, root),
        DoctorCommand::LargeFiles(command) => large_files::run(command, root),
    }
}
