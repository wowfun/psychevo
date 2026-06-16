
#[derive(Debug, Parser, Default)]
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
    #[arg(long, help = "Disable agent discovery and the spawn_agent tool")]
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
    #[command(about = "List, add, and diagnose external agent backends")]
    Backend(AgentBackendArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct AgentBackendArgs {
    #[command(subcommand)]
    pub(crate) command: AgentBackendCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum AgentBackendCommand {
    #[command(about = "List configured external agent backends")]
    List(AgentBackendListArgs),
    #[command(about = "Add a generic ACP backend registration")]
    Add(AgentBackendAddArgs),
    #[command(about = "Run local diagnostics for one backend")]
    Doctor(AgentBackendDoctorArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct AgentBackendListArgs {
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct AgentBackendAddArgs {
    #[arg(
        value_name = "ID",
        help = "Backend id; also used as generated agent name"
    )]
    pub(crate) id: String,
    #[arg(
        long,
        value_name = "COMMAND",
        help = "Executable command to start the ACP agent"
    )]
    pub(crate) command: String,
    #[arg(
        long,
        value_name = "TEXT",
        help = "Optional generated agent description"
    )]
    pub(crate) description: Option<String>,
    #[arg(
        long,
        value_name = "LABEL",
        help = "Human display label; defaults to the id"
    )]
    pub(crate) label: Option<String>,
    #[arg(
        long = "arg",
        value_name = "ARG",
        help = "Argument passed to the backend command; repeatable"
    )]
    pub(crate) args: Vec<String>,
    #[arg(
        long = "entrypoint",
        value_name = "peer|subagent",
        help = "Supported entrypoint; repeatable, defaults to peer and subagent"
    )]
    pub(crate) entrypoints: Vec<String>,
    #[arg(
        long = "client-capability",
        value_name = "CAP",
        help = "Client callback capability; repeatable, defaults to fs.read, fs.write, terminal"
    )]
    pub(crate) client_capabilities: Vec<String>,
    #[arg(
        long,
        conflicts_with = "local",
        help = "Write global config; this is the default"
    )]
    pub(crate) global: bool,
    #[arg(
        long,
        conflicts_with = "global",
        help = "Write current workdir .psychevo/config.toml"
    )]
    pub(crate) local: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct AgentBackendDoctorArgs {
    #[arg(value_name = "ID", help = "Backend id")]
    pub(crate) id: String,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
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
