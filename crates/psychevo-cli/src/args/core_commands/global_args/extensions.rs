use clap::Parser;

#[derive(Debug, Parser)]
pub(crate) struct ExtensionInstallArgs {
    #[arg(
        value_name = "SOURCE",
        help = "First-party Extension id, HTTPS release descriptor, or local directory"
    )]
    pub(crate) source: String,
    #[arg(
        short = 'l',
        long = "local",
        help = "Install for the current workspace"
    )]
    pub(crate) local: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ExtensionRemoveArgs {
    #[arg(value_name = "SELECTOR", help = "Installed Extension id")]
    pub(crate) selector: String,
    #[arg(
        short = 'l',
        long = "local",
        help = "Remove from the current workspace"
    )]
    pub(crate) local: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ExtensionListArgs {
    #[arg(long = "local", help = "List only current-workspace Extensions")]
    pub(crate) local: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ExtensionUpdateArgs {
    #[arg(
        value_name = "SELECTOR",
        conflicts_with_all = ["extensions", "all"],
        help = "Update one installed remote Extension"
    )]
    pub(crate) selector: Option<String>,
    #[arg(
        long,
        conflicts_with_all = ["selector", "all"],
        help = "Update every installed remote Extension"
    )]
    pub(crate) extensions: bool,
    #[arg(
        long,
        conflicts_with_all = ["selector", "extensions"],
        help = "Update pevo, then every installed remote Extension"
    )]
    pub(crate) all: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}
