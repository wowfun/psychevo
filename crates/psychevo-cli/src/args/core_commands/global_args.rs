pub(in crate::args) mod basic_profile;
pub(in crate::args) mod desktop;
pub(in crate::args) mod extensions;
pub(in crate::args) mod gateway;
pub(in crate::args) mod plugins_hooks;
pub(in crate::args) mod run_stats_context;
pub(in crate::args) mod skills;
pub(in crate::args) mod skills_entry;

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::args::admin_commands::profile_args::{AuthArgs, ConfigArgs, ModelArgs, SessionArgs};
#[cfg(feature = "gateway")]
use crate::args::core_commands::command_variants::TuiArgs;
use crate::args::core_commands::command_variants::{AgentArgs, ToolArgs};
#[cfg(feature = "acp")]
use basic_profile::AcpArgs;
#[cfg(feature = "gateway")]
use basic_profile::ServeArgs;
use basic_profile::{DoctorArgs, InitArgs, McpArgs, ProfileArgs, SetupArgs};
#[cfg(feature = "desktop")]
use desktop::DesktopArgs;
use extensions::{
    ExtensionInstallArgs, ExtensionListArgs, ExtensionRemoveArgs, ExtensionUpdateArgs,
};
#[cfg(feature = "gateway")]
use gateway::{GatewayArgs, WebArgs};
use plugins_hooks::{HooksArgs, PluginArgs};
#[cfg(feature = "gateway")]
use run_stats_context::RunArgs;
use run_stats_context::{ContextArgs, StatsArgs};
use skills_entry::SkillsArgs;

#[derive(Debug, Parser)]
#[command(name = "pevo", version)]
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
    #[arg(
        short = 'C',
        long = "cd",
        value_name = "DIR",
        help = "Open the TUI or GUI with this workspace cwd"
    )]
    pub(crate) cd: Option<PathBuf>,
    #[arg(
        short = 'e',
        value_name = "LOCAL_PATH",
        action = clap::ArgAction::Append,
        help = "Temporarily load an in-place Extension for this invocation"
    )]
    pub(crate) extension_paths: Vec<PathBuf>,
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
    #[command(about = "Install and immediately enable an Extension")]
    Install(ExtensionInstallArgs),
    #[command(about = "Remove an installed Extension while retaining its data")]
    Remove(ExtensionRemoveArgs),
    #[command(about = "List installed Extensions")]
    List(ExtensionListArgs),
    #[command(about = "Update pevo or installed Extensions")]
    Update(ExtensionUpdateArgs),
    #[command(about = "List, trust, enable, and disable local hooks")]
    Hooks(HooksArgs),
    #[command(about = "List and configure local toolsets")]
    Tool(ToolArgs),
    #[command(
        about = "Run one coding-agent turn",
        long_about = "Run one coding-agent turn through the configured provider. The turn can read stdin, use local tools in the selected cwd, write session state to SQLite, and include discovered or explicit skills unless disabled."
    )]
    #[cfg(feature = "gateway")]
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
    #[cfg(feature = "acp")]
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
    #[cfg(feature = "gateway")]
    Tui(TuiArgs),
    #[command(
        about = "Open or manage the managed local Web UI",
        long_about = "Open the managed local Web UI for the current cwd, or start, stop, and restart the managed Web server. The default command is equivalent to `pevo gateway open` and emits exactly one JSON object on stdout."
    )]
    #[cfg(feature = "gateway")]
    Web(WebArgs),
    #[command(
        about = "Open the native Desktop app from a source checkout",
        long_about = "Open the native Desktop app from a Psychevo source checkout by running the existing @psychevo/desktop Tauri development entrypoint."
    )]
    #[cfg(feature = "desktop")]
    Desktop(DesktopArgs),
    #[command(
        about = "Run the headless local Gateway API server",
        long_about = "Run the headless local Gateway API server on loopback. The command emits one ready JSON object on stdout and writes logs to stderr."
    )]
    #[cfg(feature = "gateway")]
    Serve(ServeArgs),
    #[command(
        about = "Manage the local Gateway Web Shell",
        long_about = "Open, start, inspect, stop, or restart the managed Gateway Web Shell. The default subcommand is open."
    )]
    #[cfg(feature = "gateway")]
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
    #[command(external_subcommand)]
    External(Vec<OsString>),
}
