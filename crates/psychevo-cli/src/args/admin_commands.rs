#[allow(unused_imports)]
pub(crate) use super::*;

#[derive(Debug, Parser)]
pub(crate) struct AgentStatusArgs {
    #[arg(
        long,
        help = "Show agents across every session instead of the latest session tree"
    )]
    pub(crate) all: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct AgentInspectArgs {
    #[arg(value_name = "ID", help = "Agent id, task name, or child session id")]
    pub(crate) id: String,
    #[arg(
        long,
        value_name = "N",
        default_value_t = 20,
        help = "Maximum recent transcript rows to include"
    )]
    pub(crate) limit: usize,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct AgentWaitArgs {
    #[arg(
        long,
        value_name = "MS",
        default_value_t = 30_000,
        help = "Maximum wait time"
    )]
    pub(crate) timeout_ms: u64,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct AgentIdArgs {
    #[arg(value_name = "ID", help = "Agent id, task name, or child session id")]
    pub(crate) id: String,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct AgentSendArgs {
    #[arg(value_name = "ID", help = "Agent id, task name, or child session id")]
    pub(crate) id: String,
    #[arg(required = true, num_args = 1.., value_name = "MESSAGE", help = "Message to queue")]
    pub(crate) message: Vec<String>,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct AgentLogsArgs {
    #[arg(value_name = "ID", help = "Agent id, task name, or child session id")]
    pub(crate) id: String,
    #[arg(
        long,
        value_name = "N",
        default_value_t = 20,
        help = "Maximum transcript rows to print"
    )]
    pub(crate) limit: usize,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
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
    #[command(about = "Rebuild one session prompt prefix from current local context")]
    ReloadContext(SessionIdArgs),
    #[command(
        about = "Export selected local session sections",
        long_about = "Export selected local session sections from SQLite without contacting providers. The last-provider-request include is unredacted and may expose hidden prompts, project instructions, skill context, tool schemas, tool outputs, and image data URLs. The last-provider-response include is a normalized persisted response projection, not raw provider bytes."
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
    #[arg(short = 'f', long, value_enum, value_name = "FORMAT", default_value_t = SessionExportFormatArg::Markdown, help = "Artifact format to write")]
    pub(crate) format: SessionExportFormatArg,
    #[arg(
        short = 'o',
        long,
        value_name = "PATH",
        help = "Write the artifact to this path instead of stdout"
    )]
    pub(crate) output: Option<PathBuf>,
    #[arg(short = 'i', long = "include", value_name = "LIST", value_parser = parse_export_include_arg, help = "Comma-separated sections: header/h, messages/m, reasoning/r, provider-input-evidence/pie, last-provider-request/lpr, last-provider-response")]
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
        about = "Set the default model in scoped config",
        long_about = "Set the default provider-qualified model in config TOML. Without a scope flag, this writes the current workdir .psychevo config; use -g/--global to write the global Psychevo home config. This command does not contact providers."
    )]
    Set(ModelSetArgs),
    #[command(
        about = "Fetch provider model catalogs",
        long_about = "Fetch model catalogs from configured provider /models endpoints and cache them locally. This is the only model command that contacts providers."
    )]
    Fetch(ModelFetchArgs),
}

pub(crate) fn parse_export_include_arg(value: &str) -> std::result::Result<String, String> {
    parse_include_arg(value, SessionArtifactKind::Export)
}

pub(crate) fn parse_share_include_arg(value: &str) -> std::result::Result<String, String> {
    parse_include_arg(value, SessionArtifactKind::Share)
}

pub(crate) fn parse_include_arg(
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
pub(crate) struct ModelSetArgs {
    #[arg(value_name = "PROVIDER/MODEL", help = "Provider-qualified model")]
    pub(crate) model: String,
    #[arg(
        short = 'g',
        long = "global",
        conflicts_with = "local",
        help = "Write to the global Psychevo home config"
    )]
    pub(crate) global: bool,
    #[arg(
        long,
        conflicts_with = "global",
        help = "Write to the current workdir .psychevo config"
    )]
    pub(crate) local: bool,
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
    #[command(about = "Open the project or global config in $EDITOR")]
    Edit(ConfigEditArgs),
    #[command(about = "Set a TOML config value by dot path")]
    Set(ConfigSetArgs),
    #[command(about = "Validate global, local, or effective config")]
    Validate(ConfigShowArgs),
    #[command(about = "Show config diagnostics")]
    Doctor(ConfigShowArgs),
    #[command(about = "Show concise config status")]
    Status(ConfigShowArgs),
    #[command(about = "Inspect and add provider configuration")]
    Provider(ConfigProviderArgs),
    #[command(about = "List and remove project-local permission rules")]
    Permissions(ConfigPermissionsArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct ConfigJsonArgs {
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ConfigShowArgs {
    #[arg(short = 'g', long = "global", conflicts_with_all = ["local", "effective"], help = "Read the global Psychevo home config")]
    pub(crate) global: bool,
    #[arg(long, conflicts_with_all = ["global", "effective"], help = "Read the current workdir .psychevo config")]
    pub(crate) local: bool,
    #[arg(long, conflicts_with_all = ["global", "local"], help = "Show the effective merged configuration")]
    pub(crate) effective: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ConfigEditArgs {
    #[arg(
        short = 'g',
        long = "global",
        help = "Edit the global Psychevo home config instead of the current workdir config"
    )]
    pub(crate) global: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ConfigSetArgs {
    #[arg(
        value_name = "KEY",
        help = "Dot-path config key, for example approval_policy"
    )]
    pub(crate) key: String,
    #[arg(
        value_name = "VALUE",
        help = "TOML literal value, for example \"on-request\", true, or [\"cargo\"]"
    )]
    pub(crate) value: String,
    #[arg(
        short = 'g',
        long = "global",
        help = "Write the global config instead of the current workdir config"
    )]
    pub(crate) global: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ConfigProviderArgs {
    #[command(subcommand)]
    pub(crate) command: ConfigProviderCommand,
}

#[derive(Debug, Parser)]
pub(crate) struct ConfigPermissionsArgs {
    #[command(subcommand)]
    pub(crate) command: ConfigPermissionsCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ConfigPermissionsCommand {
    #[command(about = "List project-local permission rules")]
    List(ConfigJsonArgs),
    #[command(about = "Remove one project-local permission rule")]
    Remove(ConfigPermissionRemoveArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct ConfigPermissionRemoveArgs {
    #[arg(
        long,
        value_enum,
        value_name = "KIND",
        help = "Rule list: allow, ask, or deny"
    )]
    pub(crate) kind: PermissionRuleKindArg,
    #[arg(long, value_name = "RULE", help = "Exact rule string to remove")]
    pub(crate) rule: String,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ConfigProviderCommand {
    #[command(about = "List configured providers")]
    List(ConfigShowArgs),
    #[command(
        about = "Add an OpenAI-compatible provider",
        long_about = "Add an OpenAI-compatible provider to global or local config. Provider settings are written to config TOML; --api-key-stdin reads one secret from stdin and writes it to the selected .env instead of accepting a raw key in argv."
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
        long = "no-auth",
        conflicts_with_all = ["api_key_env", "api_key_stdin"],
        help = "Configure the provider without API-key authentication"
    )]
    pub(crate) no_auth: bool,
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
pub(crate) struct AuthArgs {
    #[command(subcommand)]
    pub(crate) command: AuthCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum AuthCommand {
    #[command(about = "Show provider credential status without printing secrets")]
    Status(AuthStatusArgs),
    #[command(about = "Configure a provider and default model")]
    Setup(AuthSetupArgs),
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
pub(crate) struct AuthSetupArgs {
    #[arg(long, value_name = "PROVIDER", help = "Provider id to configure")]
    pub(crate) provider: String,
    #[arg(long, value_name = "MODEL", help = "Model id for the provider")]
    pub(crate) model: String,
    #[arg(
        long = "base-url",
        value_name = "URL",
        help = "OpenAI-compatible base URL"
    )]
    pub(crate) base_url: Option<String>,
    #[arg(
        long = "api-kind",
        default_value = "openai-compatible",
        help = "Provider API kind; only openai-compatible is supported"
    )]
    pub(crate) api_kind: String,
    #[arg(long, value_name = "ENV", help = "API key environment variable name")]
    pub(crate) api_key_env: Option<String>,
    #[arg(long = "api-key-stdin", help = "Read one API key from piped stdin")]
    pub(crate) api_key_stdin: bool,
    #[arg(
        long = "no-auth",
        conflicts_with_all = ["api_key_env", "api_key_stdin"],
        help = "Configure provider without API-key authentication"
    )]
    pub(crate) no_auth: bool,
    #[arg(short = 'g', long = "global", conflicts_with = "local")]
    pub(crate) global: bool,
    #[arg(long, conflicts_with = "global")]
    pub(crate) local: bool,
    #[arg(long = "fetch", conflicts_with = "no_fetch", default_value_t = false)]
    pub(crate) fetch: bool,
    #[arg(long = "no-fetch")]
    pub(crate) no_fetch: bool,
    #[arg(long, value_name = "TEXT", help = "Provider label")]
    pub(crate) label: Option<String>,
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
        short = 'g',
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
pub(crate) enum PermissionModeArg {
    Default,
    #[value(name = "acceptEdits", alias = "accept-edits")]
    AcceptEdits,
    Plan,
    #[value(name = "dontAsk", alias = "dont-ask")]
    DontAsk,
    #[value(name = "bypassPermissions", alias = "bypass-permissions")]
    BypassPermissions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum PermissionRuleKindArg {
    Allow,
    Ask,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ToolModeArg {
    Plan,
    Default,
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

impl PermissionModeArg {
    pub(crate) fn run_mode(self) -> RunMode {
        match self {
            Self::Plan => RunMode::Plan,
            _ => RunMode::Default,
        }
    }

    pub(crate) fn permission_mode(self) -> PermissionMode {
        match self {
            Self::Default | Self::Plan => PermissionMode::Default,
            Self::AcceptEdits => PermissionMode::AcceptEdits,
            Self::DontAsk => PermissionMode::DontAsk,
            Self::BypassPermissions => PermissionMode::BypassPermissions,
        }
    }
}

impl ToolModeArg {
    pub(crate) fn run_mode(self) -> RunMode {
        match self {
            Self::Plan => RunMode::Plan,
            Self::Default => RunMode::Default,
        }
    }
}

impl PermissionRuleKindArg {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Ask => "ask",
            Self::Deny => "deny",
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::*;

    #[test]
    fn parses_singular_skill_and_rejects_plural_skills() {
        let cli = Cli::try_parse_from(["pevo", "skill", "list", "--json"]).expect("skill");
        assert!(matches!(
            cli.command,
            Commands::Skill(SkillsArgs {
                command: Some(SkillsCommand::List(SkillsListArgs { json: true, .. }))
            })
        ));
        assert!(Cli::try_parse_from(["pevo", "skills", "list"]).is_err());
    }

    #[test]
    fn parses_local_scope_and_rejects_project_alias() {
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
                command: Some(SkillsCommand::Install(SkillsInstallArgs {
                    local: true,
                    force: true,
                    ..
                }))
            })
        ));
        assert!(
            Cli::try_parse_from(["pevo", "skill", "install", "/tmp/reviewer", "--project"])
                .is_err()
        );
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
                "messages,reasoning,provider-input-evidence,last-provider-request,last-provider-response",
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "pevo",
                "session",
                "export",
                "latest",
                "-i",
                "h,m,r,pie,lpr,last-provider-response",
            ])
            .is_ok()
        );
        assert!(Cli::try_parse_from(["pevo", "run", "-f", "json", "hello"]).is_ok());
        assert!(
            Cli::try_parse_from(["pevo", "run", "--permission-mode", "dontAsk", "hello"]).is_ok()
        );
        assert!(
            Cli::try_parse_from(["pevo", "run", "--dangerously-skip-permissions", "hello"]).is_ok()
        );
        assert!(
            Cli::try_parse_from(["pevo", "agent", "run", "reviewer", "-f", "json", "hello"])
                .is_ok()
        );
        assert!(Cli::try_parse_from(["pevo", "session", "export", "latest", "-f", "json"]).is_ok());
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
            Cli::try_parse_from([
                "pevo",
                "session",
                "export",
                "latest",
                "--include",
                "last-raw-response",
            ])
            .is_err()
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
                "pevo",
                "session",
                "share",
                "latest",
                "--include",
                "last-provider-response",
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
        assert!(Cli::try_parse_from(["pevo", "model", "set", "mock/model", "--json"]).is_ok());
        assert!(Cli::try_parse_from(["pevo", "model", "set", "-g", "mock/model"]).is_ok());
        assert!(Cli::try_parse_from(["pevo", "config", "show", "--local", "--json"]).is_ok());
        assert!(Cli::try_parse_from(["pevo", "config", "show", "-g", "--json"]).is_ok());
        assert!(Cli::try_parse_from(["pevo", "config", "permissions", "list", "--json"]).is_ok());
        assert!(
            Cli::try_parse_from([
                "pevo",
                "config",
                "permissions",
                "remove",
                "--kind",
                "allow",
                "--rule",
                "ExecCommand(npm test *)",
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from(["pevo", "auth", "set", "mock", "--api-key-stdin", "--local"])
                .is_ok()
        );
    }
}
