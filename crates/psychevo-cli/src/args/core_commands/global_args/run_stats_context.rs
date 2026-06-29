#[derive(Debug, Parser)]
pub(crate) struct RunArgs {
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
        long = "runtime",
        value_name = "ID",
        help = "Run through a configured runtime backend instead of the native runtime"
    )]
    pub(crate) runtime: Option<String>,
    #[arg(
        long = "runtime-option",
        value_name = "KEY=VALUE",
        value_parser = parse_runtime_option_arg,
        help = "Set a current-runtime option for this run; repeatable"
    )]
    pub(crate) runtime_option: Vec<(String, String)>,
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
        help = "Continue the latest run session for the selected cwd"
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
    #[arg(long, help = "Disable agent discovery and the spawn_agent tool")]
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

pub(crate) fn parse_runtime_option_arg(
    value: &str,
) -> std::result::Result<(String, String), String> {
    let Some((key, option_value)) = value.split_once('=') else {
        return Err("runtime option must use KEY=VALUE form".to_string());
    };
    let key = key.trim();
    if key.is_empty() {
        return Err("runtime option key must not be empty".to_string());
    }
    Ok((key.to_string(), option_value.trim().to_string()))
}

#[derive(Debug, Parser)]
pub(crate) struct StatsArgs {
    #[arg(
        long = "dir",
        value_name = "DIR",
        help = "Report usage for this cwd instead of the current directory"
    )]
    pub(crate) dir: Option<PathBuf>,
    #[arg(
        long,
        help = "Report usage across all cwds in the local state database"
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
        help = "Session id or latest active session for the selected cwd"
    )]
    pub(crate) session: Option<String>,
    #[arg(
        long = "dir",
        value_name = "DIR",
        help = "Resolve latest session relative to this cwd"
    )]
    pub(crate) dir: Option<PathBuf>,
    #[arg(long, help = "Emit a structured context_snapshot JSON object")]
    pub(crate) json: bool,
}
