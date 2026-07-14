#[derive(Debug, Parser)]
pub(crate) struct AcpArgs {
    #[arg(
        long,
        help = "Print provider setup guidance instead of starting the ACP server"
    )]
    pub(crate) setup: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct McpArgs {
    #[command(subcommand)]
    pub(crate) command: McpCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum McpCommand {
    #[command(about = "Run the Psychevo MCP stdio server")]
    Serve(McpServeArgs),
}

#[derive(Debug, Parser, Clone)]
pub(crate) struct McpServeArgs {
    #[arg(
        long = "dir",
        value_name = "DIR",
        help = "Run tools and resolve project config from this cwd"
    )]
    pub(crate) dir: Option<PathBuf>,
    #[arg(
        short = 'm',
        long,
        value_name = "PROVIDER/MODEL",
        help = "Use this provider-qualified model for MCP-triggered turns"
    )]
    pub(crate) model: Option<String>,
    #[arg(
        long,
        value_enum,
        value_name = "VARIANT",
        help = "Override reasoning effort for MCP-triggered turns"
    )]
    pub(crate) variant: Option<VariantArg>,
    #[arg(
        long = "permission-mode",
        value_enum,
        value_name = "MODE",
        help = "Override permission mode: default, acceptEdits, plan, dontAsk, or bypassPermissions"
    )]
    pub(crate) permission_mode: Option<PermissionModeArg>,
    #[arg(
        long = "dangerously-skip-permissions",
        conflicts_with = "permission_mode",
        help = "Skip prompt-level permission prompts for MCP-triggered turns; hard denies still apply"
    )]
    pub(crate) dangerously_skip_permissions: bool,
    #[arg(
        long = "project-context",
        value_enum,
        value_name = "MODE",
        conflicts_with = "isolated",
        help = "Override project instruction discovery: git-root, cwd, or off"
    )]
    pub(crate) project_context: Option<ProjectContextArg>,
    #[arg(
        long,
        conflicts_with = "project_context",
        help = "Alias for --project-context cwd"
    )]
    pub(crate) isolated: bool,
    #[arg(
        long,
        value_name = "NAME_OR_PATH",
        conflicts_with = "no_agents",
        help = "Run MCP-triggered turns with a selected agent definition"
    )]
    pub(crate) agent: Option<String>,
    #[arg(long, help = "Disable agent discovery and the spawn_agent tool")]
    pub(crate) no_agents: bool,
    #[arg(long, help = "Disable default and configured skill discovery")]
    pub(crate) no_skills: bool,
    #[arg(
        long = "skill",
        value_name = "NAME_OR_PATH",
        help = "Add an explicit skill by name or filesystem path; repeatable"
    )]
    pub(crate) skill: Vec<String>,
}

#[derive(Debug, Parser)]
pub(crate) struct InitArgs {
    #[arg(
        long,
        help = "Back up existing SQLite state files and create a fresh state database"
    )]
    pub(crate) reset_state: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ProfileArgs {
    #[command(subcommand)]
    pub(crate) command: Option<ProfileCommand>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ProfileCommand {
    #[command(about = "List local Psychevo profiles")]
    List(ProfileListArgs),
    #[command(about = "Show one local Psychevo profile")]
    Show(ProfileShowArgs),
    #[command(about = "Create a named Psychevo profile")]
    Create(ProfileCreateArgs),
    #[command(about = "Set the sticky active Psychevo profile")]
    Use(ProfileUseArgs),
    #[command(about = "Delete a named Psychevo profile")]
    Delete(ProfileDeleteArgs),
    #[command(about = "Rename a named Psychevo profile")]
    Rename(ProfileRenameArgs),
    #[command(about = "Create or remove a shell alias for a profile")]
    Alias(ProfileAliasArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct ProfileListArgs {
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ProfileShowArgs {
    #[arg(value_name = "NAME", help = "Profile to inspect; defaults to active")]
    pub(crate) name: Option<String>,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ProfileCreateArgs {
    #[arg(value_name = "NAME", help = "Profile name to create")]
    pub(crate) name: String,
    #[arg(long, value_name = "TEXT", help = "Profile description")]
    pub(crate) description: Option<String>,
    #[arg(
        long,
        help = "Clone config, .env, skills, and agents from another profile"
    )]
    pub(crate) clone: bool,
    #[arg(
        long = "clone-from",
        value_name = "NAME",
        help = "Profile to clone from; defaults to the active profile"
    )]
    pub(crate) clone_from: Option<String>,
    #[arg(
        long,
        value_name = "COMMAND",
        num_args = 0..=1,
        default_missing_value = "",
        help = "Create a shell alias; without a value uses the profile name"
    )]
    pub(crate) alias: Option<String>,
}

#[derive(Debug, Parser)]
pub(crate) struct ProfileUseArgs {
    #[arg(value_name = "NAME", help = "Profile name, or `default`")]
    pub(crate) name: String,
}

#[derive(Debug, Parser)]
pub(crate) struct ProfileDeleteArgs {
    #[arg(value_name = "NAME", help = "Named profile to delete")]
    pub(crate) name: String,
    #[arg(long, help = "Confirm profile deletion without prompting")]
    pub(crate) yes: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ProfileRenameArgs {
    #[arg(value_name = "OLD", help = "Existing profile name")]
    pub(crate) old: String,
    #[arg(value_name = "NEW", help = "New profile name")]
    pub(crate) new: String,
}

#[derive(Debug, Parser)]
pub(crate) struct ProfileAliasArgs {
    #[arg(value_name = "NAME", help = "Profile to alias")]
    pub(crate) profile: String,
    #[arg(long, value_name = "COMMAND", help = "Alias command name")]
    pub(crate) name: Option<String>,
    #[arg(long, help = "Remove the alias instead of creating it")]
    pub(crate) remove: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct DoctorArgs {
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
    #[arg(
        long,
        help = "Opt in to live provider/model checks that may contact configured providers"
    )]
    pub(crate) live: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SetupArgs {
    #[arg(
        long,
        help = "Print the setup steps without prompting or writing files"
    )]
    pub(crate) dry_run: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ServeArgs {
    #[arg(
        long = "dir",
        value_name = "DIR",
        help = "Use this default cwd for API requests without an explicit scope"
    )]
    pub(crate) dir: Option<PathBuf>,
    #[arg(
        long,
        value_name = "ADDR",
        default_value = "127.0.0.1:0",
        help = "Loopback address for the local Gateway server"
    )]
    pub(crate) bind: std::net::SocketAddr,
    #[arg(
        long = "token-file",
        value_name = "FILE",
        help = "Read the Bearer API token from this file"
    )]
    pub(crate) token_file: Option<PathBuf>,
    #[arg(
        long = "internal-static-dir",
        hide = true,
        value_name = "DIR",
        help = "Serve Workbench assets from this directory"
    )]
    pub(crate) static_dir: Option<PathBuf>,
    #[arg(
        long = "internal-managed-state",
        hide = true,
        value_name = "FILE",
        help = "Write managed server metadata to this file after binding"
    )]
    pub(crate) managed_state: Option<PathBuf>,
    #[arg(
        long = "internal-managed-instance",
        hide = true,
        value_name = "UUID",
        help = "Identify the managed Gateway instance"
    )]
    pub(crate) managed_instance: Option<String>,
    #[arg(
        long = "internal-managed-lease",
        hide = true,
        value_name = "FILE",
        help = "Hold the managed Gateway instance lease until process exit"
    )]
    pub(crate) managed_lease: Option<PathBuf>,
    #[arg(
        long = "internal-bind-fallbacks",
        hide = true,
        default_value_t = 0,
        help = "Try this many sequential ports after --bind when the address is already in use"
    )]
    pub(crate) bind_fallbacks: u16,
}
