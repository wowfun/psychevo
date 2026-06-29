#[derive(Debug, Subcommand)]
pub(crate) enum SkillsCommand {
    #[command(about = "List discoverable skills")]
    List(SkillsListArgs),
    #[command(about = "Show a skill manifest or file")]
    View(SkillsViewArgs),
    #[command(about = "Browse installed or cached hub skills")]
    Browse(SkillsQueryArgs),
    #[command(about = "Search installed or cached hub skills")]
    Search(SkillsQueryArgs),
    #[command(about = "Inspect a hub or installed skill")]
    Inspect(SkillsInspectArgs),
    #[command(about = "Install one or more skills from a source path or hub identifier")]
    Install(SkillsInstallArgs),
    #[command(about = "Check installed hub-managed skills for updates")]
    Check(SkillsJsonArgs),
    #[command(about = "Update installed hub-managed skills")]
    Update(SkillsJsonArgs),
    #[command(about = "Audit a skill package or all installed skills")]
    Audit(SkillsAuditArgs),
    #[command(about = "Uninstall a local skill")]
    Uninstall(SkillsNameArgs),
    #[command(about = "Publish a skill through the configured hub backend")]
    Publish(SkillsPublishArgs),
    #[command(about = "Inspect or update skill configuration")]
    Config(SkillsConfigArgs),
    #[command(about = "Manage local skill bundles")]
    Bundle(SkillsBundleArgs),
    #[command(about = "Manage hub snapshots (CLI only)")]
    Snapshot(SkillsJsonArgs),
    #[command(about = "Manage hub taps (CLI only)")]
    Tap(SkillsJsonArgs),
    #[command(about = "Reset bundled skill manifest state")]
    Reset(SkillsResetArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsListArgs {
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
    #[arg(long, help = "Include disabled skills in the listing")]
    pub(crate) all: bool,
    #[arg(long, help = "Include richer readiness and package metadata")]
    pub(crate) detail: bool,
    #[arg(long, value_name = "CATEGORY", help = "Filter by skill category")]
    pub(crate) category: Option<String>,
    #[arg(long, value_name = "SOURCE", help = "Filter by discovery source")]
    pub(crate) source: Option<String>,
    #[arg(long, help = "Only list model-enabled skills")]
    pub(crate) enabled_only: bool,
    #[arg(long, value_name = "PLATFORM", help = "Filter by supported platform")]
    pub(crate) platform: Option<String>,
    #[arg(long, value_name = "TAG", help = "Filter by tag")]
    pub(crate) tag: Option<String>,
    #[arg(long, value_name = "STATUS", help = "Filter by readiness status")]
    pub(crate) readiness: Option<String>,
    #[arg(
        long,
        value_name = "MODE",
        help = "Sort mode: category, name, usage, recent"
    )]
    pub(crate) sort: Option<String>,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsViewArgs {
    #[arg(value_name = "NAME", help = "Skill name to inspect")]
    pub(crate) name: String,
    #[arg(
        value_name = "FILE",
        help = "Optional file inside the skill package to print"
    )]
    pub(crate) file_path: Option<String>,
    #[arg(long, help = "Emit structured JSON instead of file content")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsQueryArgs {
    #[arg(value_name = "QUERY", help = "Optional search text")]
    pub(crate) query: Option<String>,
    #[arg(
        long,
        value_name = "N",
        default_value_t = 20,
        help = "Maximum rows to return"
    )]
    pub(crate) limit: usize,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsInspectArgs {
    #[arg(
        value_name = "IDENTIFIER",
        help = "Hub identifier or installed skill name"
    )]
    pub(crate) identifier: String,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsNameArgs {
    #[arg(value_name = "NAME", help = "Skill name")]
    pub(crate) name: String,
    #[arg(
        short = 'g',
        long = "global",
        conflicts_with = "local",
        help = "Apply the change in the global Psychevo home"
    )]
    pub(crate) global: bool,
    #[arg(
        long,
        conflicts_with = "global",
        help = "Apply the change in the current cwd .psychevo scope"
    )]
    pub(crate) local: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsJsonArgs {
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsNameScopeArgs {
    #[arg(value_name = "NAME", help = "Skill name")]
    pub(crate) name: String,
    #[arg(
        short = 'g',
        long = "global",
        conflicts_with = "local",
        help = "Apply the change in the global Psychevo home"
    )]
    pub(crate) global: bool,
    #[arg(
        long = "local",
        conflicts_with = "global",
        help = "Apply the change in the current cwd .psychevo scope"
    )]
    pub(crate) local: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsInstallArgs {
    #[arg(
        value_name = "SOURCE",
        help = "Directory containing one skill or a collection of skills"
    )]
    pub(crate) source: String,
    #[arg(
        long,
        value_name = "NAME",
        help = "Install a named skill from a collection source"
    )]
    pub(crate) name: Option<String>,
    #[arg(
        long,
        conflicts_with = "name",
        help = "Install every skill found in the source"
    )]
    pub(crate) all: bool,
    #[arg(
        short = 'g',
        long = "global",
        conflicts_with = "local",
        help = "Install under the global Psychevo home"
    )]
    pub(crate) global: bool,
    #[arg(
        long = "local",
        conflicts_with = "global",
        help = "Install under the current cwd .psychevo scope"
    )]
    pub(crate) local: bool,
    #[arg(long, help = "Overwrite an existing installed skill")]
    pub(crate) force: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsAuditArgs {
    #[arg(
        value_name = "PATH",
        help = "Optional directory to audit for skill packages"
    )]
    pub(crate) path: Option<PathBuf>,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsPublishArgs {
    #[arg(value_name = "PATH", help = "Skill directory to publish")]
    pub(crate) path: PathBuf,
    #[arg(long, value_name = "REPO", help = "GitHub repository override")]
    pub(crate) repo: Option<String>,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsResetArgs {
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsConfigArgs {
    #[command(subcommand)]
    pub(crate) command: SkillsConfigCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum SkillsConfigCommand {
    #[command(about = "Show effective skill configuration status")]
    Status(SkillsJsonArgs),
    #[command(about = "Enable a skill in the selected scope")]
    Enable(SkillsNameScopeArgs),
    #[command(about = "Disable a skill in the selected scope")]
    Disable(SkillsNameScopeArgs),
    #[command(about = "Set a skills.config.* value")]
    Set(SkillsConfigSetArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsConfigSetArgs {
    #[arg(value_name = "KEY", help = "Config key under skills.config.*")]
    pub(crate) key: String,
    #[arg(value_name = "VALUE", help = "JSON value or string value")]
    pub(crate) value: String,
    #[arg(
        short = 'g',
        long = "global",
        conflicts_with = "local",
        help = "Write under the global Psychevo home"
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
pub(crate) struct SkillsBundleArgs {
    #[command(subcommand)]
    pub(crate) command: SkillsBundleCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum SkillsBundleCommand {
    #[command(about = "List skill bundles")]
    List(SkillsJsonArgs),
    #[command(about = "Show one skill bundle")]
    Show(SkillsBundleNameArgs),
    #[command(about = "Create or update a skill bundle")]
    Create(SkillsBundleCreateArgs),
    #[command(about = "Delete a skill bundle")]
    Delete(SkillsBundleDeleteArgs),
    #[command(about = "Reload bundle files")]
    Reload(SkillsJsonArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsBundleNameArgs {
    #[arg(value_name = "NAME", help = "Bundle name or slug")]
    pub(crate) name: String,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsBundleCreateArgs {
    #[arg(value_name = "NAME", help = "Bundle name")]
    pub(crate) name: String,
    #[arg(
        long = "skill",
        value_name = "NAME",
        required = true,
        help = "Member skill name; repeatable"
    )]
    pub(crate) skills: Vec<String>,
    #[arg(long, value_name = "TEXT", help = "Bundle description")]
    pub(crate) description: Option<String>,
    #[arg(long, value_name = "TEXT", help = "Bundle instruction")]
    pub(crate) instruction: Option<String>,
    #[arg(
        short = 'g',
        long = "global",
        conflicts_with = "local",
        help = "Write under the global Psychevo home"
    )]
    pub(crate) global: bool,
    #[arg(
        long = "local",
        conflicts_with = "global",
        help = "Write under the current cwd .psychevo scope"
    )]
    pub(crate) local: bool,
    #[arg(long, help = "Overwrite an existing bundle file")]
    pub(crate) force: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsBundleDeleteArgs {
    #[arg(value_name = "NAME", help = "Bundle name or slug")]
    pub(crate) name: String,
    #[arg(
        short = 'g',
        long = "global",
        conflicts_with = "local",
        help = "Delete from the global Psychevo home"
    )]
    pub(crate) global: bool,
    #[arg(
        long = "local",
        conflicts_with = "global",
        help = "Delete from the current cwd .psychevo scope"
    )]
    pub(crate) local: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}
