#[derive(Debug, Parser, Clone)]
pub(crate) struct DesktopArgs {
    #[arg(
        short = 'C',
        long = "cd",
        value_name = "DIR",
        help = "Open Desktop with this fallback workspace cwd"
    )]
    pub(crate) cd: Option<PathBuf>,
}
