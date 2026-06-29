#[derive(Debug, Parser)]
pub(crate) struct PluginArgs {
    #[command(subcommand)]
    pub(crate) command: Option<PluginCommand>,
}

#[derive(Debug, Parser)]
pub(crate) struct HooksArgs {
    #[command(subcommand)]
    pub(crate) command: Option<HooksCommand>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum HooksCommand {
    #[command(about = "List discovered hooks")]
    List(HooksListArgs),
    #[command(about = "Trust the current hash for one hook")]
    Trust(HookKeyArgs),
    #[command(about = "Enable one hook in profile hook state")]
    Enable(HookKeyArgs),
    #[command(about = "Disable one hook in profile hook state")]
    Disable(HookKeyArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct HooksListArgs {
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct HookKeyArgs {
    #[arg(value_name = "HOOK_KEY", help = "Hook key from `pevo hooks list`")]
    pub(crate) key: String,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Subcommand)]
pub(crate) enum PluginCommand {
    #[command(about = "List installed plugins")]
    List(PluginListArgs),
    #[command(about = "Show one installed plugin")]
    View(PluginViewArgs),
    #[command(about = "Diagnose installed plugins")]
    Doctor(PluginDoctorArgs),
    #[command(about = "Install a plugin package from a local directory or Git source")]
    Install(PluginInstallArgs),
    #[command(about = "Uninstall a plugin from the selected scope")]
    Uninstall(PluginNameScopeArgs),
    #[command(about = "Enable a plugin in the selected scope")]
    Enable(PluginNameScopeArgs),
    #[command(about = "Disable a plugin in the selected scope")]
    Disable(PluginNameScopeArgs),
    #[command(about = "Manage local plugin marketplace source catalogs")]
    Marketplace(PluginMarketplaceArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct PluginListArgs {
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct PluginViewArgs {
    #[arg(value_name = "NAME_OR_NAME_AT_SOURCE", help = "Plugin selector")]
    pub(crate) selector: String,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct PluginDoctorArgs {
    #[arg(
        value_name = "NAME_OR_NAME_AT_SOURCE",
        help = "Optional plugin selector"
    )]
    pub(crate) selector: Option<String>,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct PluginInstallArgs {
    #[arg(value_name = "SOURCE", help = "Local plugin directory or Git source")]
    pub(crate) source: String,
    #[arg(
        long = "ref",
        value_name = "REF",
        help = "Git ref to checkout for Git sources"
    )]
    pub(crate) git_ref: Option<String>,
    #[arg(
        short = 'g',
        long = "global",
        conflicts_with = "local",
        help = "Install under the active profile home"
    )]
    pub(crate) global: bool,
    #[arg(
        long = "local",
        conflicts_with = "global",
        help = "Install under the current cwd .psychevo scope"
    )]
    pub(crate) local: bool,
    #[arg(
        long,
        help = "Replace an existing installed package from the same source"
    )]
    pub(crate) force: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct PluginNameScopeArgs {
    #[arg(value_name = "NAME_OR_NAME_AT_SOURCE", help = "Plugin selector")]
    pub(crate) selector: String,
    #[arg(
        short = 'g',
        long = "global",
        conflicts_with = "local",
        help = "Write under the active profile home"
    )]
    pub(crate) global: bool,
    #[arg(
        long = "local",
        conflicts_with = "global",
        help = "Write under the current cwd .psychevo scope"
    )]
    pub(crate) local: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct PluginMarketplaceArgs {
    #[command(subcommand)]
    pub(crate) command: PluginMarketplaceCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum PluginMarketplaceCommand {
    #[command(about = "List plugin marketplace source catalogs")]
    List(PluginMarketplaceListArgs),
    #[command(about = "Add a local or Git marketplace source catalog entry")]
    Add(PluginMarketplaceAddArgs),
    #[command(about = "Remove a marketplace source catalog entry")]
    Remove(PluginMarketplaceRemoveArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct PluginMarketplaceListArgs {
    #[arg(short = 'g', long = "global", conflicts_with = "local")]
    pub(crate) global: bool,
    #[arg(long = "local", conflicts_with = "global")]
    pub(crate) local: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct PluginMarketplaceAddArgs {
    #[arg(value_name = "NAME", help = "Catalog entry name")]
    pub(crate) name: String,
    #[arg(value_name = "SOURCE", help = "Local directory or Git source")]
    pub(crate) source: String,
    #[arg(
        long,
        value_name = "local|git",
        default_value = "local",
        help = "Source kind"
    )]
    pub(crate) kind: String,
    #[arg(long = "ref", value_name = "REF", help = "Optional Git ref")]
    pub(crate) git_ref: Option<String>,
    #[arg(short = 'g', long = "global", conflicts_with = "local")]
    pub(crate) global: bool,
    #[arg(long = "local", conflicts_with = "global")]
    pub(crate) local: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct PluginMarketplaceRemoveArgs {
    #[arg(value_name = "NAME", help = "Catalog entry name")]
    pub(crate) name: String,
    #[arg(short = 'g', long = "global", conflicts_with = "local")]
    pub(crate) global: bool,
    #[arg(long = "local", conflicts_with = "global")]
    pub(crate) local: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}
