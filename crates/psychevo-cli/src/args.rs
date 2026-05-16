use std::path::PathBuf;

use clap::{Parser, Subcommand};
use psychevo_runtime::{SessionArtifactKind, SessionExportIncludeSet, SmokeControl};

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
    #[command(about = "List, view, create, install, and toggle local skills")]
    Skill(SkillsArgs),
    #[command(
        about = "Run deterministic fake-provider smoke behavior",
        long_about = "Run deterministic fake-provider smoke behavior for development and validation. This command uses explicit local db/workdir paths and does not contact live providers."
    )]
    Smoke(SmokeArgs),
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
        about = "Open the fullscreen terminal UI",
        long_about = "Open the fullscreen terminal UI for interactive coding-agent work. In non-terminal stdin/stdout, each input line is processed deterministically as a prompt, slash command, or shell escape."
    )]
    Tui(TuiArgs),
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
pub(crate) struct SmokeArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "SQLite database path for deterministic smoke state"
    )]
    pub(crate) db: PathBuf,
    #[arg(
        long,
        value_name = "DIR",
        help = "Workdir used by the fake-provider smoke run"
    )]
    pub(crate) workdir: PathBuf,
    #[arg(
        long,
        value_name = "ID",
        help = "Existing smoke session id to continue"
    )]
    pub(crate) session: Option<String>,
    #[arg(
        long,
        value_name = "TEXT",
        help = "Prompt text for the fake-provider turn"
    )]
    pub(crate) prompt: Option<String>,
    #[arg(
        long,
        value_name = "N",
        help = "Maximum prior messages to include in fake context"
    )]
    pub(crate) max_context_messages: Option<usize>,
    #[arg(long, value_enum, default_value_t = ControlArg::None, help = "Inject deterministic control behavior into the smoke run")]
    pub(crate) control: ControlArg,
    #[arg(long, help = "Reset smoke state before running")]
    pub(crate) reset: bool,
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
    #[arg(long, value_enum, value_name = "FORMAT", default_value_t = RunFormatArg::Default, help = "Select human output or NDJSON machine output")]
    pub(crate) format: RunFormatArg,
    #[arg(
        long,
        help = "Include reasoning_delta events in JSON output; sanitized messages stay reasoning-free"
    )]
    pub(crate) include_reasoning: bool,
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
    pub(crate) command: SkillsCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum SkillsCommand {
    #[command(about = "List discoverable skills")]
    List(SkillsListArgs),
    #[command(about = "Show a skill manifest or file")]
    View(SkillsViewArgs),
    #[command(about = "Create a local or global skill skeleton")]
    Create(SkillsCreateArgs),
    #[command(about = "Patch text in an existing skill file")]
    Patch(SkillsPatchArgs),
    #[command(about = "Remove a local skill")]
    Remove(SkillsNameArgs),
    #[command(about = "Enable a skill in the selected scope")]
    Enable(SkillsNameScopeArgs),
    #[command(about = "Disable a skill in the selected scope")]
    Disable(SkillsNameScopeArgs),
    #[command(about = "Install one or more skills from a source path")]
    Install(SkillsInstallArgs),
    #[command(about = "Scan a directory for skill packages")]
    Scan(SkillsScanArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsListArgs {
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
    #[arg(long, help = "Include disabled skills in the listing")]
    pub(crate) all: bool,
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
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsCreateArgs {
    #[arg(value_name = "NAME", help = "New skill name")]
    pub(crate) name: String,
    #[arg(long, value_name = "TEXT", help = "Human-readable skill description")]
    pub(crate) description: String,
    #[arg(
        long = "global",
        conflicts_with = "local",
        help = "Create under the global Psychevo home"
    )]
    pub(crate) global: bool,
    #[arg(
        long,
        conflicts_with = "global",
        help = "Create under the current workdir .psychevo scope"
    )]
    pub(crate) local: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsPatchArgs {
    #[arg(value_name = "NAME", help = "Skill name to patch")]
    pub(crate) name: String,
    #[arg(long, value_name = "TEXT", help = "Existing text to replace")]
    pub(crate) old: String,
    #[arg(long, value_name = "TEXT", help = "Replacement text")]
    pub(crate) new: String,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsNameArgs {
    #[arg(value_name = "NAME", help = "Skill name")]
    pub(crate) name: String,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsNameScopeArgs {
    #[arg(value_name = "NAME", help = "Skill name")]
    pub(crate) name: String,
    #[arg(
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
        long = "global",
        conflicts_with = "local",
        help = "Install under the global Psychevo home"
    )]
    pub(crate) global: bool,
    #[arg(
        long,
        conflicts_with = "global",
        help = "Install under the current workdir .psychevo scope"
    )]
    pub(crate) local: bool,
    #[arg(long, help = "Overwrite an existing installed skill")]
    pub(crate) force: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsScanArgs {
    #[arg(value_name = "PATH", help = "Directory to scan for skill packages")]
    pub(crate) path: PathBuf,
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
pub(crate) struct SessionArgs {
    #[command(subcommand)]
    pub(crate) command: SessionCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum SessionCommand {
    #[command(about = "List sessions for the current workdir")]
    List(SessionListArgs),
    #[command(about = "Show one session summary")]
    Show(SessionIdArgs),
    #[command(about = "Rename one session")]
    Rename(SessionRenameArgs),
    #[command(about = "Archive one active session")]
    Archive(SessionIdArgs),
    #[command(about = "Restore one archived session")]
    Restore(SessionIdArgs),
    #[command(
        about = "Export selected local session sections",
        long_about = "Export selected local session sections from SQLite without contacting providers. The last-provider-request include is unredacted and may expose hidden prompts, project instructions, skill context, tool schemas, tool outputs, and image data URLs."
    )]
    Export(SessionExportArgs),
    #[command(
        about = "Write a local shareable Markdown artifact",
        long_about = "Write a local shareable Markdown artifact for a session. This is a local packaging step only: it does not upload content, create public links, or include reconstructed provider request bodies."
    )]
    Share(SessionShareArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct SessionListArgs {
    #[arg(long, help = "List archived sessions instead of active sessions")]
    pub(crate) archived: bool,
    #[arg(
        long,
        value_name = "N",
        default_value_t = 20,
        help = "Maximum sessions to show"
    )]
    pub(crate) limit: usize,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SessionIdArgs {
    #[arg(
        value_name = "SESSION_OR_LATEST",
        help = "Exact session id or latest for this workdir"
    )]
    pub(crate) session: String,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SessionRenameArgs {
    #[arg(
        value_name = "SESSION_OR_LATEST",
        help = "Exact session id or latest for this workdir"
    )]
    pub(crate) session: String,
    #[arg(required = true, num_args = 1.., value_name = "TITLE", help = "New session title; words are joined with spaces")]
    pub(crate) title: Vec<String>,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SessionExportArgs {
    #[arg(
        value_name = "SESSION_OR_LATEST",
        help = "Exact session id or latest for this workdir"
    )]
    pub(crate) session: String,
    #[arg(long, value_enum, value_name = "FORMAT", default_value_t = SessionExportFormatArg::Markdown, help = "Artifact format to write")]
    pub(crate) format: SessionExportFormatArg,
    #[arg(
        short = 'o',
        long,
        value_name = "PATH",
        help = "Write the artifact to this path instead of stdout"
    )]
    pub(crate) output: Option<PathBuf>,
    #[arg(short = 'i', long = "include", value_name = "LIST", value_parser = parse_export_include_arg, help = "Comma-separated sections: header/h, messages/m, reasoning/r, provider-input-evidence/pie, last-provider-request/lpr")]
    pub(crate) include: Option<String>,
}

#[derive(Debug, Parser)]
pub(crate) struct SessionShareArgs {
    #[arg(
        value_name = "SESSION_OR_LATEST",
        help = "Exact session id or latest for this workdir"
    )]
    pub(crate) session: String,
    #[arg(
        short = 'o',
        long,
        value_name = "PATH",
        help = "Write the Markdown share artifact to this path"
    )]
    pub(crate) output: Option<PathBuf>,
    #[arg(short = 'i', long = "include", value_name = "LIST", value_parser = parse_share_include_arg, help = "Comma-separated sections: header/h, messages/m, reasoning/r, provider-input-evidence/pie")]
    pub(crate) include: Option<String>,
    #[arg(
        long,
        help = "Emit structured JSON for the command result; artifact remains Markdown"
    )]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ModelArgs {
    #[command(subcommand)]
    pub(crate) command: ModelCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ModelCommand {
    #[command(about = "List locally configured or cached models")]
    List(ModelListArgs),
    #[command(about = "Show the model selected by configuration and overrides")]
    Current(ModelJsonArgs),
    #[command(
        about = "Fetch provider model catalogs",
        long_about = "Fetch model catalogs from configured provider /models endpoints and cache them locally. This is the only model command that contacts providers."
    )]
    Fetch(ModelFetchArgs),
}

fn parse_export_include_arg(value: &str) -> std::result::Result<String, String> {
    parse_include_arg(value, SessionArtifactKind::Export)
}

fn parse_share_include_arg(value: &str) -> std::result::Result<String, String> {
    parse_include_arg(value, SessionArtifactKind::Share)
}

fn parse_include_arg(
    value: &str,
    artifact_kind: SessionArtifactKind,
) -> std::result::Result<String, String> {
    SessionExportIncludeSet::parse(value, artifact_kind)
        .map(|_| value.to_string())
        .map_err(|err| err.to_string())
}

#[derive(Debug, Parser)]
pub(crate) struct ModelListArgs {
    #[arg(value_name = "PROVIDER", help = "Optional provider id to filter")]
    pub(crate) provider: Option<String>,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ModelJsonArgs {
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ModelFetchArgs {
    #[arg(
        value_name = "PROVIDER",
        help = "Optional provider id; omitted fetches all fetchable providers"
    )]
    pub(crate) provider: Option<String>,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ConfigArgs {
    #[command(subcommand)]
    pub(crate) command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ConfigCommand {
    #[command(about = "Show resolved Psychevo path locations")]
    Path(ConfigJsonArgs),
    #[command(about = "Show global, local, or effective config")]
    Show(ConfigShowArgs),
    #[command(about = "Inspect and add provider configuration")]
    Provider(ConfigProviderArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct ConfigJsonArgs {
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ConfigShowArgs {
    #[arg(long = "global", conflicts_with_all = ["local", "effective"], help = "Read the global Psychevo home config")]
    pub(crate) global: bool,
    #[arg(long, conflicts_with_all = ["global", "effective"], help = "Read the current workdir .psychevo config")]
    pub(crate) local: bool,
    #[arg(long, conflicts_with_all = ["global", "local"], help = "Show the effective merged configuration")]
    pub(crate) effective: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ConfigProviderArgs {
    #[command(subcommand)]
    pub(crate) command: ConfigProviderCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ConfigProviderCommand {
    #[command(about = "List configured providers")]
    List(ConfigShowArgs),
    #[command(
        about = "Add an OpenAI-compatible provider",
        long_about = "Add an OpenAI-compatible provider to global or local config. Provider settings are written to config JSONC; --api-key-stdin reads one secret from stdin and writes it to the selected .env instead of accepting a raw key in argv."
    )]
    Add(ConfigProviderAddArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct ConfigProviderAddArgs {
    #[arg(
        long,
        value_name = "ID",
        help = "Provider id used in provider/model names"
    )]
    pub(crate) id: String,
    #[arg(long, value_name = "TEXT", help = "Human-readable provider label")]
    pub(crate) label: String,
    #[arg(
        long = "base-url",
        value_name = "URL",
        help = "OpenAI-compatible API base URL"
    )]
    pub(crate) base_url: String,
    #[arg(
        long = "api-key-env",
        value_name = "ENV",
        help = "Environment variable name that will hold the provider API key"
    )]
    pub(crate) api_key_env: Option<String>,
    #[arg(
        long = "api-key-stdin",
        help = "Read one API key from stdin and write it to the selected .env"
    )]
    pub(crate) api_key_stdin: bool,
    #[arg(
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
pub(crate) struct AuthArgs {
    #[command(subcommand)]
    pub(crate) command: AuthCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum AuthCommand {
    #[command(about = "Show provider credential status without printing secrets")]
    Status(AuthStatusArgs),
    #[command(
        about = "Read and store a provider API key",
        long_about = "Read a provider API key from stdin and write it to the selected .env scope. Raw API keys are never accepted as argv values or printed in command output."
    )]
    Set(AuthSetArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct AuthStatusArgs {
    #[arg(value_name = "PROVIDER", help = "Optional provider id to inspect")]
    pub(crate) provider: Option<String>,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct AuthSetArgs {
    #[arg(
        value_name = "PROVIDER",
        help = "Provider id whose API key should be written"
    )]
    pub(crate) provider: String,
    #[arg(long = "api-key-stdin", help = "Read one API key from stdin")]
    pub(crate) api_key_stdin: bool,
    #[arg(
        long = "global",
        conflicts_with = "local",
        help = "Write to the global Psychevo home .env"
    )]
    pub(crate) global: bool,
    #[arg(
        long,
        conflicts_with = "global",
        help = "Write to the current workdir .psychevo/.env"
    )]
    pub(crate) local: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum ControlArg {
    None,
    StopAfterTurn,
    AbortOnAgentStart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum VariantArg {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum RunFormatArg {
    Default,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum SessionExportFormatArg {
    Markdown,
    Json,
}

impl From<ControlArg> for SmokeControl {
    fn from(value: ControlArg) -> Self {
        match value {
            ControlArg::None => SmokeControl::None,
            ControlArg::StopAfterTurn => SmokeControl::StopAfterTurn,
            ControlArg::AbortOnAgentStart => SmokeControl::AbortOnAgentStart,
        }
    }
}

impl VariantArg {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            VariantArg::None => "none",
            VariantArg::Minimal => "minimal",
            VariantArg::Low => "low",
            VariantArg::Medium => "medium",
            VariantArg::High => "high",
            VariantArg::Xhigh => "xhigh",
            VariantArg::Max => "max",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_singular_skill_and_rejects_plural_skills() {
        let cli = Cli::try_parse_from(["pevo", "skill", "list", "--json"]).expect("skill");
        assert!(matches!(
            cli.command,
            Commands::Skill(SkillsArgs {
                command: SkillsCommand::List(SkillsListArgs { json: true, .. })
            })
        ));
        assert!(Cli::try_parse_from(["pevo", "skills", "list"]).is_err());
    }

    #[test]
    fn parses_local_scope_and_rejects_project_scope() {
        let cli = Cli::try_parse_from([
            "pevo",
            "skill",
            "install",
            "/tmp/reviewer",
            "--local",
            "--force",
        ])
        .expect("local skill install");
        assert!(matches!(
            cli.command,
            Commands::Skill(SkillsArgs {
                command: SkillsCommand::Install(SkillsInstallArgs {
                    local: true,
                    force: true,
                    ..
                })
            })
        ));
        assert!(
            Cli::try_parse_from([
                "pevo",
                "config",
                "provider",
                "add",
                "--id",
                "mock",
                "--label",
                "Mock",
                "--base-url",
                "http://127.0.0.1/v1",
                "--project",
            ])
            .is_err()
        );
    }

    #[test]
    fn parses_new_cli_command_families() {
        assert!(Cli::try_parse_from(["pevo", "session", "list", "--archived", "--json"]).is_ok());
        assert!(
            Cli::try_parse_from([
                "pevo",
                "session",
                "export",
                "latest",
                "--format",
                "json",
                "--include",
                "messages,reasoning,provider-input-evidence,last-provider-request",
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from(["pevo", "session", "export", "latest", "-i", "h,m,r,pie,lpr",])
                .is_ok()
        );
        assert!(
            Cli::try_parse_from(["pevo", "session", "export", "latest", "--with-reasoning"])
                .is_err()
        );
        assert!(
            Cli::try_parse_from(["pevo", "session", "export", "latest", "--full-inputs"]).is_err()
        );
        assert!(
            Cli::try_parse_from(["pevo", "session", "export", "latest", "--last-request"]).is_err()
        );
        assert!(
            Cli::try_parse_from(["pevo", "session", "share", "latest", "-i", "h,m,r,pie",]).is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "pevo",
                "session",
                "share",
                "latest",
                "--include",
                "header,messages,reasoning,provider-input-evidence",
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from(["pevo", "session", "export", "latest", "--raw-requests"]).is_err()
        );
        assert!(
            Cli::try_parse_from(["pevo", "session", "share", "latest", "--last-request"]).is_err()
        );
        assert!(
            Cli::try_parse_from(["pevo", "session", "share", "latest", "--with-reasoning"])
                .is_err()
        );
        assert!(
            Cli::try_parse_from(["pevo", "session", "share", "latest", "--full-inputs"]).is_err()
        );
        assert!(
            Cli::try_parse_from([
                "pevo",
                "session",
                "share",
                "latest",
                "--include",
                "last-provider-request",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "pevo", "session", "share", "latest", "--output", "share.md", "--json",
            ])
            .is_ok()
        );
        assert!(Cli::try_parse_from(["pevo", "model", "fetch", "mock", "--json"]).is_ok());
        assert!(Cli::try_parse_from(["pevo", "config", "show", "--local", "--json"]).is_ok());
        assert!(
            Cli::try_parse_from(["pevo", "auth", "set", "mock", "--api-key-stdin", "--local"])
                .is_ok()
        );
    }
}
