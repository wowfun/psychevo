#[allow(unused_imports)]
pub(crate) use super::*;
#[derive(Debug, Parser)]
#[command(name = "pevo")]
#[command(
    about = "Local coding-agent CLI and terminal UI",
    long_about = "pevo runs Psychevo coding-agent tasks, opens the fullscreen terminal UI, and manages local sessions, skills, models, configuration, credentials, and usage data."
)]
pub(crate) struct Cli {
    #[arg(
        short = 'p',
        long,
        global = true,
        value_name = "NAME",
        help = "Use a named Psychevo profile for this invocation"
    )]
    pub(crate) profile: Option<String>,
    #[command(subcommand)]
    pub(crate) command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    #[command(about = "Create or repair the active Psychevo profile home")]
    Init(InitArgs),
    #[command(about = "List, inspect, create, switch, and manage local profiles")]
    Profile(ProfileArgs),
    #[command(about = "List, inspect, run, and manage agents", alias = "agents")]
    Agent(AgentArgs),
    #[command(about = "List, view, create, install, and toggle local skills")]
    Skill(SkillsArgs),
    #[command(about = "List, inspect, install, and enable local plugins")]
    Plugin(PluginArgs),
    #[command(about = "List, trust, enable, and disable local hooks")]
    Hooks(HooksArgs),
    #[command(about = "List and configure local toolsets")]
    Tool(ToolArgs),
    #[command(
        about = "Run one coding-agent turn",
        long_about = "Run one coding-agent turn through the configured provider. The turn can read stdin, use local tools in the selected cwd, write session state to SQLite, and include discovered or explicit skills unless disabled."
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
        about = "Run the Model Context Protocol stdio server",
        long_about = "Run Psychevo as a minimal MCP stdio server exposing the psychevo and psychevo-reply tools for MCP-speaking clients."
    )]
    Mcp(McpArgs),
    #[command(
        about = "Open the fullscreen terminal UI",
        long_about = "Open the fullscreen terminal UI for interactive coding-agent work. In non-terminal stdin/stdout, each input line is processed deterministically as a prompt, slash command, or shell escape."
    )]
    Tui(TuiArgs),
    #[command(
        about = "Open or manage the managed local Web UI",
        long_about = "Open the managed local Web UI for the current cwd, or start, stop, and restart the managed Web server. The default command is equivalent to `pevo gateway open` and emits exactly one JSON object on stdout."
    )]
    Web(WebArgs),
    #[command(
        about = "Open the native Desktop app from a source checkout",
        long_about = "Open the native Desktop app from a Psychevo source checkout by running the existing @psychevo/desktop Tauri development entrypoint."
    )]
    Desktop(DesktopArgs),
    #[command(
        about = "Run the headless local Gateway API server",
        long_about = "Run the headless local Gateway API server on loopback. The command emits one ready JSON object on stdout and writes logs to stderr."
    )]
    Serve(ServeArgs),
    #[command(
        about = "Manage the local Gateway Web Shell",
        long_about = "Open, start, inspect, stop, or restart the managed Gateway Web Shell. The default subcommand is open."
    )]
    Gateway(GatewayArgs),
    #[command(
        about = "Run local deterministic diagnostics",
        long_about = "Run local diagnostics for Psychevo home, config, auth, model selection, Web UI assets, Gateway status, and local tools. Provider network checks run only with --live."
    )]
    Doctor(DoctorArgs),
    #[command(
        about = "Run the interactive first-run setup wizard",
        long_about = "Run a TTY-only setup wizard that initializes Psychevo home, configures a provider/model, optionally stores an API key, checks Web UI assets, and finishes with a doctor summary."
    )]
    Setup(SetupArgs),
}

include!("global_args/basic_profile.rs");
include!("global_args/desktop.rs");
include!("global_args/gateway.rs");
include!("global_args/run_stats_context.rs");
include!("global_args/skills_entry.rs");
include!("global_args/plugins_hooks.rs");
include!("global_args/skills.rs");
