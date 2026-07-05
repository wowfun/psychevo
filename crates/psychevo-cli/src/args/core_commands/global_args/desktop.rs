#[derive(Debug, Parser, Clone)]
pub(crate) struct DesktopArgs {
    #[arg(
        long = "dir",
        value_name = "DIR",
        help = "Open Desktop with this fallback workspace cwd"
    )]
    pub(crate) dir: Option<PathBuf>,
}
