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
    #[arg()]
    pub(crate) message: Vec<String>,
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
