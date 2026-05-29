#[allow(unused_imports)]
pub(crate) use super::*;
#[derive(Debug, Parser)]
#[command(name = "pevo")]
#[command(
    about = "Local coding-agent CLI and terminal UI",
    long_about = "pevo runs Psychevo coding-agent tasks, opens the fullscreen terminal UI, and manages local sessions, skills, models, configuration, credentials, and usage data."
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    #[command(about = "Create or repair the global Psychevo home")]
    Init(InitArgs),
    #[command(about = "List, inspect, run, and manage agents")]
    Agent(AgentArgs),
    #[command(about = "List, view, create, install, and toggle local skills")]
    Skill(SkillsArgs),
    #[command(about = "List and configure local toolsets")]
    Tool(ToolArgs),
    #[command(
        about = "Run one coding-agent turn",
        long_about = "Run one coding-agent turn through the configured provider. The turn can read stdin, use local tools in the selected workdir, write session state to SQLite, and include discovered or explicit skills unless disabled."
    )]
    Run(RunArgs),
    #[command(about = "Show local usage and estimated cost from SQLite state")]
    Stats(StatsArgs),
    #[command(about = "Inspect local context-window usage for a session")]
    Context(ContextArgs),
    #[command(about = "List, inspect, rename, archive, restore, export, or share local sessions")]
    Session(SessionArgs),
    #[command(about = "Inspect configured models and fetch provider model catalogs")]
    Model(ModelArgs),
    #[command(about = "Inspect paths/config and add provider configuration")]
    Config(ConfigArgs),
    #[command(about = "Inspect credential status and write provider API keys")]
    Auth(AuthArgs),
    #[command(
        about = "Run the Agent Client Protocol stdio server",
        long_about = "Run Psychevo as an Agent Client Protocol stdio server for ACP-speaking editors and clients."
    )]
    Acp(AcpArgs),
    #[command(
        about = "Open the fullscreen terminal UI",
        long_about = "Open the fullscreen terminal UI for interactive coding-agent work. In non-terminal stdin/stdout, each input line is processed deterministically as a prompt, slash command, or shell escape."
    )]
    Tui(TuiArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct AcpArgs {
    #[arg(
        long,
        help = "Print provider setup guidance instead of starting the ACP server"
    )]
    pub(crate) setup: bool,
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
pub(crate) struct RunArgs {
    #[arg(
        long = "dir",
        value_name = "DIR",
        help = "Run tools and resolve project config from this workdir"
    )]
    pub(crate) dir: Option<PathBuf>,
    #[arg(
        short = 'm',
        long,
        value_name = "PROVIDER/MODEL",
        help = "Use this provider-qualified model for this run"
    )]
    pub(crate) model: Option<String>,
    #[arg(
        long,
        value_enum,
        value_name = "VARIANT",
        help = "Override reasoning effort for this run"
    )]
    pub(crate) variant: Option<VariantArg>,
    #[arg(
        short = 's',
        long,
        value_name = "ID",
        conflicts_with = "continue_latest",
        help = "Continue an explicit session id"
    )]
    pub(crate) session: Option<String>,
    #[arg(
        short = 'c',
        long = "continue",
        conflicts_with = "session",
        help = "Continue the latest run session for the selected workdir"
    )]
    pub(crate) continue_latest: bool,
    #[arg(short = 'f', long, value_enum, value_name = "FORMAT", default_value_t = RunFormatArg::Default, help = "Select human output or NDJSON machine output")]
    pub(crate) format: RunFormatArg,
    #[arg(
        long,
        help = "Include reasoning_delta events in JSON output; sanitized messages stay reasoning-free"
    )]
    pub(crate) include_reasoning: bool,
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
        help = "Skip prompt-level permission prompts for this run; hard denies still apply"
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
        help = "Run this turn with a selected agent definition"
    )]
    pub(crate) agent: Option<String>,
    #[arg(long, help = "Disable agent discovery and the Agent tool")]
    pub(crate) no_agents: bool,
    #[arg(
        long,
        help = "Disable default and configured skill discovery for this run"
    )]
    pub(crate) no_skills: bool,
    #[arg(
        long = "skill",
        value_name = "NAME_OR_PATH",
        help = "Add an explicit skill by name or filesystem path; repeatable"
    )]
    pub(crate) skill: Vec<String>,
    #[arg(
        value_name = "MESSAGE",
        help = "Prompt text; multiple words are joined and stdin is appended when present"
    )]
    pub(crate) message: Vec<String>,
}

#[derive(Debug, Parser)]
pub(crate) struct StatsArgs {
    #[arg(
        long = "dir",
        value_name = "DIR",
        help = "Report usage for this workdir instead of the current directory"
    )]
    pub(crate) dir: Option<PathBuf>,
    #[arg(
        long,
        help = "Report usage across all workdirs in the local state database"
    )]
    pub(crate) all: bool,
    #[arg(
        long,
        value_name = "DAYS",
        help = "Limit results to sessions updated within this many days"
    )]
    pub(crate) days: Option<u64>,
    #[arg(
        long,
        value_name = "N",
        default_value_t = 10,
        help = "Maximum rows for top model/tool/session breakdowns"
    )]
    pub(crate) limit: usize,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ContextArgs {
    #[arg(
        long = "session",
        value_name = "ID_OR_LATEST",
        help = "Session id or latest active session for the selected workdir"
    )]
    pub(crate) session: Option<String>,
    #[arg(
        long = "dir",
        value_name = "DIR",
        help = "Resolve latest session relative to this workdir"
    )]
    pub(crate) dir: Option<PathBuf>,
    #[arg(long, help = "Emit a structured context_snapshot JSON object")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsArgs {
    #[command(subcommand)]
    pub(crate) command: Option<SkillsCommand>,
}

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
        help = "Apply the change in the current workdir .psychevo scope"
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
        help = "Apply the change in the current workdir .psychevo scope"
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
        help = "Install under the current workdir .psychevo scope"
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
        help = "Write under the current workdir .psychevo scope"
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
        help = "Write under the current workdir .psychevo scope"
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
        help = "Delete from the current workdir .psychevo scope"
    )]
    pub(crate) local: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct TuiArgs {
    #[arg(
        long = "dir",
        value_name = "DIR",
        help = "Open the TUI for this workdir"
    )]
    pub(crate) dir: Option<PathBuf>,
    #[arg(
        short = 'm',
        long,
        value_name = "PROVIDER/MODEL",
        help = "Use this provider-qualified model for this TUI process"
    )]
    pub(crate) model: Option<String>,
    #[arg(
        long,
        value_enum,
        value_name = "VARIANT",
        help = "Override reasoning effort for this TUI process"
    )]
    pub(crate) variant: Option<VariantArg>,
    #[arg(
        long = "permission-mode",
        value_enum,
        value_name = "MODE",
        help = "Initial permission mode: default, acceptEdits, plan, dontAsk, or bypassPermissions"
    )]
    pub(crate) permission_mode: Option<PermissionModeArg>,
    #[arg(
        short = 's',
        long,
        value_name = "ID",
        help = "Start from an explicit session id"
    )]
    pub(crate) session: Option<String>,
    #[arg(
        long = "new",
        help = "Start a new session on the first submitted prompt"
    )]
    pub(crate) new_session: bool,
    #[arg(
        long,
        help = "Show local debug projections such as usage and allowlisted provider metadata"
    )]
    pub(crate) debug: bool,
    #[arg(long, help = "Disable default and configured skill discovery")]
    pub(crate) no_skills: bool,
    #[arg(
        long,
        value_name = "NAME_OR_PATH",
        conflicts_with = "no_agents",
        help = "Use this agent as the main TUI session identity"
    )]
    pub(crate) agent: Option<String>,
    #[arg(long, help = "Disable agent discovery and the Agent tool")]
    pub(crate) no_agents: bool,
    #[arg(
        long = "skill",
        value_name = "NAME_OR_PATH",
        help = "Add an explicit skill by name or filesystem path; repeatable"
    )]
    pub(crate) skill: Vec<String>,
    #[arg(
        value_name = "MESSAGE",
        help = "Initial prompt; leading ! runs a local shell escape instead"
    )]
    pub(crate) message: Vec<String>,
}

#[derive(Debug, Parser)]
pub(crate) struct ToolArgs {
    #[command(subcommand)]
    pub(crate) command: Option<ToolCommand>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ToolCommand {
    #[command(about = "List effective toolsets and tools")]
    List(ToolListArgs),
    #[command(about = "Show one toolset")]
    Show(ToolShowArgs),
    #[command(about = "Enable a toolset for one mode")]
    Enable(ToolModeMutationArgs),
    #[command(about = "Disable a toolset for one mode")]
    Disable(ToolModeMutationArgs),
    #[command(about = "Create or overwrite a scoped custom toolset")]
    Create(ToolCreateArgs),
    #[command(about = "Remove a scoped custom toolset")]
    Remove(ToolRemoveArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct ToolListArgs {
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ToolShowArgs {
    #[arg(value_name = "NAME", help = "Toolset name")]
    pub(crate) name: String,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ToolModeMutationArgs {
    #[arg(value_name = "NAME", help = "Toolset name")]
    pub(crate) name: String,
    #[arg(long, value_enum, default_value_t = ToolModeArg::Default, help = "Mode to change: default or plan")]
    pub(crate) mode: ToolModeArg,
    #[arg(
        short = 'g',
        long = "global",
        conflicts_with = "local",
        help = "Write to the global Psychevo home scope"
    )]
    pub(crate) global: bool,
    #[arg(
        long,
        conflicts_with = "global",
        help = "Write to the current workdir .psychevo scope"
    )]
    pub(crate) local: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ToolCreateArgs {
    #[arg(value_name = "NAME", help = "Custom toolset name")]
    pub(crate) name: String,
    #[arg(long, value_name = "TEXT", help = "Toolset description")]
    pub(crate) description: Option<String>,
    #[arg(
        long = "tool",
        value_name = "TOOL",
        help = "Tool name to include; repeatable"
    )]
    pub(crate) tools: Vec<String>,
    #[arg(
        long = "include",
        value_name = "TOOLSET",
        help = "Toolset to include; repeatable"
    )]
    pub(crate) includes: Vec<String>,
    #[arg(
        short = 'g',
        long = "global",
        conflicts_with = "local",
        help = "Write to the global Psychevo home scope"
    )]
    pub(crate) global: bool,
    #[arg(
        long,
        conflicts_with = "global",
        help = "Write to the current workdir .psychevo scope"
    )]
    pub(crate) local: bool,
    #[arg(long, help = "Overwrite an existing custom toolset")]
    pub(crate) force: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ToolRemoveArgs {
    #[arg(value_name = "NAME", help = "Custom toolset name")]
    pub(crate) name: String,
    #[arg(
        short = 'g',
        long = "global",
        conflicts_with = "local",
        help = "Remove from the global Psychevo home scope"
    )]
    pub(crate) global: bool,
    #[arg(
        long,
        conflicts_with = "global",
        help = "Remove from the current workdir .psychevo scope"
    )]
    pub(crate) local: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct AgentArgs {
    #[command(subcommand)]
    pub(crate) command: AgentCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum AgentCommand {
    #[command(about = "List discoverable agents")]
    List(AgentListArgs),
    #[command(about = "Show one agent definition")]
    View(AgentNameArgs),
    #[command(about = "Validate one agent definition")]
    Validate(AgentNameArgs),
    #[command(about = "Run one prompt with a selected agent")]
    Run(AgentRunArgs),
    #[command(about = "Show live and resumable agent runs")]
    Status(AgentStatusArgs),
    #[command(about = "Inspect one child-agent session")]
    Inspect(AgentInspectArgs),
    #[command(about = "Wait for one or more agent runs")]
    Wait(AgentWaitArgs),
    #[command(about = "Close an agent run and its descendants")]
    Close(AgentIdArgs),
    #[command(about = "Resume a closed agent run")]
    Resume(AgentIdArgs),
    #[command(about = "Send a message to an agent run")]
    Send(AgentSendArgs),
    #[command(about = "Attach to an agent run session")]
    Attach(AgentIdArgs),
    #[command(about = "Show recent transcript logs for an agent run")]
    Logs(AgentLogsArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct AgentListArgs {
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct AgentNameArgs {
    #[arg(value_name = "NAME_OR_PATH", help = "Agent name or Markdown file path")]
    pub(crate) name: String,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct AgentRunArgs {
    #[arg(value_name = "NAME_OR_PATH", help = "Agent name or Markdown file path")]
    pub(crate) name: String,
    #[arg(
        long = "dir",
        value_name = "DIR",
        help = "Run tools and resolve project config from this workdir"
    )]
    pub(crate) dir: Option<PathBuf>,
    #[arg(
        short = 'm',
        long,
        value_name = "PROVIDER/MODEL",
        help = "Use this provider-qualified model for this run"
    )]
    pub(crate) model: Option<String>,
    #[arg(
        long,
        value_enum,
        value_name = "VARIANT",
        help = "Override reasoning effort for this run"
    )]
    pub(crate) variant: Option<VariantArg>,
    #[arg(short = 'f', long, value_enum, value_name = "FORMAT", default_value_t = RunFormatArg::Default, help = "Select human output or NDJSON machine output")]
    pub(crate) format: RunFormatArg,
    #[arg(
        value_name = "MESSAGE",
        help = "Prompt text; multiple words are joined and stdin is appended when present"
    )]
    pub(crate) message: Vec<String>,
}
