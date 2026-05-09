use std::path::PathBuf;

use clap::{Parser, Subcommand};
use psychevo_runtime::SmokeControl;

#[derive(Debug, Parser)]
#[command(name = "pevo")]
#[command(about = "Psychevo command-line entrypoint")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    Init(InitArgs),
    Skills(SkillsArgs),
    Smoke(SmokeArgs),
    Run(RunArgs),
    Tui(TuiArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct InitArgs {
    #[arg(long)]
    pub(crate) reset_state: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SmokeArgs {
    #[arg(long)]
    pub(crate) db: PathBuf,
    #[arg(long)]
    pub(crate) workdir: PathBuf,
    #[arg(long)]
    pub(crate) session: Option<String>,
    #[arg(long)]
    pub(crate) prompt: Option<String>,
    #[arg(long)]
    pub(crate) max_context_messages: Option<usize>,
    #[arg(long, value_enum, default_value_t = ControlArg::None)]
    pub(crate) control: ControlArg,
    #[arg(long)]
    pub(crate) reset: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct RunArgs {
    #[arg(long = "dir")]
    pub(crate) dir: Option<PathBuf>,
    #[arg(short = 'm', long)]
    pub(crate) model: Option<String>,
    #[arg(long, value_enum)]
    pub(crate) variant: Option<VariantArg>,
    #[arg(short = 's', long, conflicts_with = "continue_latest")]
    pub(crate) session: Option<String>,
    #[arg(short = 'c', long = "continue", conflicts_with = "session")]
    pub(crate) continue_latest: bool,
    #[arg(long, value_enum, default_value_t = RunFormatArg::Default)]
    pub(crate) format: RunFormatArg,
    #[arg(long)]
    pub(crate) include_reasoning: bool,
    #[arg(long)]
    pub(crate) no_skills: bool,
    #[arg(long = "skill")]
    pub(crate) skill: Vec<String>,
    #[arg()]
    pub(crate) message: Vec<String>,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsArgs {
    #[command(subcommand)]
    pub(crate) command: SkillsCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum SkillsCommand {
    List(SkillsListArgs),
    View(SkillsViewArgs),
    Create(SkillsCreateArgs),
    Patch(SkillsPatchArgs),
    Remove(SkillsNameArgs),
    Enable(SkillsNameScopeArgs),
    Disable(SkillsNameScopeArgs),
    Install(SkillsInstallArgs),
    Scan(SkillsScanArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsListArgs {
    #[arg(long)]
    pub(crate) json: bool,
    #[arg(long)]
    pub(crate) all: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsViewArgs {
    pub(crate) name: String,
    pub(crate) file_path: Option<String>,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsCreateArgs {
    pub(crate) name: String,
    #[arg(long)]
    pub(crate) description: String,
    #[arg(long = "global", conflicts_with = "project")]
    pub(crate) global: bool,
    #[arg(long, conflicts_with = "global")]
    pub(crate) project: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsPatchArgs {
    pub(crate) name: String,
    #[arg(long)]
    pub(crate) old: String,
    #[arg(long)]
    pub(crate) new: String,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsNameArgs {
    pub(crate) name: String,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsNameScopeArgs {
    pub(crate) name: String,
    #[arg(long = "global", conflicts_with = "project")]
    pub(crate) global: bool,
    #[arg(long, conflicts_with = "global")]
    pub(crate) project: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsInstallArgs {
    pub(crate) source: String,
    #[arg(long)]
    pub(crate) name: Option<String>,
    #[arg(long, conflicts_with = "name")]
    pub(crate) all: bool,
    #[arg(long = "global", conflicts_with = "project")]
    pub(crate) global: bool,
    #[arg(long, conflicts_with = "global")]
    pub(crate) project: bool,
    #[arg(long)]
    pub(crate) force: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct SkillsScanArgs {
    pub(crate) path: PathBuf,
}

#[derive(Debug, Parser)]
pub(crate) struct TuiArgs {
    #[arg(long = "dir")]
    pub(crate) dir: Option<PathBuf>,
    #[arg(short = 'm', long)]
    pub(crate) model: Option<String>,
    #[arg(long, value_enum)]
    pub(crate) variant: Option<VariantArg>,
    #[arg(short = 's', long)]
    pub(crate) session: Option<String>,
    #[arg(long = "new")]
    pub(crate) new_session: bool,
    #[arg(long)]
    pub(crate) debug: bool,
    #[arg(long)]
    pub(crate) no_skills: bool,
    #[arg(long = "skill")]
    pub(crate) skill: Vec<String>,
    #[arg()]
    pub(crate) message: Vec<String>,
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
